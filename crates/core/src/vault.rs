//! Encrypted backups and exports, in the age format (age-encryption.org):
//! ChaCha20-Poly1305 in 64 KiB chunks, each authenticated, with the key
//! derived from a passphrase by scrypt. The files open with EraseCord, or
//! with the `age` or `rage` tools.
//!
//! A backup is written while deleting as small encrypted `tar` parts, so a
//! message is only deleted once the part holding its files is safely on
//! disk. The parts are encrypted to a key made for this backup (fast enough
//! to seal a part every few messages); that key is kept next to them,
//! encrypted with the passphrase. When the run is over, everything is
//! combined into a single archive encrypted with the passphrase itself,
//! checked, and only then are the parts removed. Nothing is ever written to
//! disk unencrypted.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use age::secrecy::ExposeSecret;
pub use age::secrecy::SecretString;
pub use age::stream::{StreamReader, StreamWriter};
use age::x25519;
use serde::{Deserialize, Serialize};

use crate::backup::{attachment_path, Backup};
use crate::client::Client;
use crate::export::{ExportFormat, ExportWriter};
use crate::job::Event;
use crate::models::Message;

const WORDS: &str = include_str!("bip39-english.txt");
/// 12 words of 2048 each: 132 bits, out of reach of guessing.
pub const PASSPHRASE_WORDS: usize = 12;
/// A part is sealed after this many messages with attachments…
pub(crate) const PART_MESSAGES: usize = 25;
/// …or this many bytes of files, whichever comes first.
pub(crate) const PART_BYTES: u64 = 100 << 20;
const KEY_FILE: &str = "key.age";
const ARCHIVE_README: &str = "EraseCord backup

attachments/   the files of your messages, in a folder per channel
messages.json  your messages with their text, links and the saved files

Opened by EraseCord (Open backup), or with the age tool and tar:
  age -d erasecord-backup-DATE.tar.age | tar x
";
const PARTS_README: &str =
    "An EraseCord backup that is still being written, or whose clean-up was stopped.

The part-*.tar.age files hold the attachments, records-*.jsonl.age the messages.
They are encrypted to a key that is itself encrypted with your passphrase (key.age).
Continue the clean-up in EraseCord to combine them into one archive, or use
Open backup in EraseCord to read them as they are.
";

/// A new passphrase of twelve random words, from the operating system's
/// secure random number generator.
pub fn generate_passphrase() -> String {
    use rand::RngExt;
    let words: Vec<&str> = WORDS.lines().collect();
    let mut rng = rand::rng();
    (0..PASSPHRASE_WORDS)
        .map(|_| words[rng.random_range(0..words.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads a passphrase from a file: its first line, without the line break.
pub fn read_passphrase_file(path: &Path) -> io::Result<SecretString> {
    let text = std::fs::read_to_string(path)?;
    let line = text
        .lines()
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r');
    if line.is_empty() {
        return Err(io::Error::other("the passphrase file is empty"));
    }
    Ok(SecretString::from(line.to_owned()))
}

pub fn secret(passphrase: &str) -> SecretString {
    SecretString::from(passphrase.to_owned())
}

/// Starts encrypting to `out` with `passphrase`. Call `finish` at the end.
pub fn encrypt<W: Write>(passphrase: &SecretString, out: W) -> io::Result<StreamWriter<W>> {
    age::Encryptor::with_user_passphrase(passphrase.clone()).wrap_output(out)
}

/// Starts decrypting `input` with `passphrase`.
pub fn decrypt<R: Read>(passphrase: &SecretString, input: R) -> Result<StreamReader<R>, String> {
    let decryptor = age::Decryptor::new(input).map_err(describe)?;
    let identity = age::scrypt::Identity::new(passphrase.clone());
    decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(describe)
}

fn decrypt_with<R: Read>(identity: &x25519::Identity, input: R) -> Result<StreamReader<R>, String> {
    let decryptor = age::Decryptor::new(input).map_err(describe)?;
    decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(describe)
}

fn describe(err: age::DecryptError) -> String {
    match err {
        age::DecryptError::DecryptionFailed | age::DecryptError::NoMatchingKeys => {
            "wrong passphrase, or the file is damaged".into()
        }
        age::DecryptError::InvalidHeader | age::DecryptError::UnknownFormat => {
            "this is not an encrypted EraseCord file".into()
        }
        other => other.to_string(),
    }
}

/// Writes `data` encrypted with `passphrase` to `path`, atomically.
fn write_encrypted(path: &Path, passphrase: &SecretString, data: &[u8]) -> io::Result<()> {
    let partial = path.with_extension("partial");
    let file = File::create(&partial)?;
    let mut writer = encrypt(passphrase, BufWriter::new(file))?;
    writer.write_all(data)?;
    let file = writer.finish()?.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    std::fs::rename(partial, path)
}

/// Where an encrypted backup is written and its public key: everything a
/// continued run needs to add to it, and nothing secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedBackupSettings {
    /// The folder of parts while the backup is being written.
    pub parts: PathBuf,
    /// The archive it becomes.
    pub archive: PathBuf,
    /// The backup's own key, public half (`age1…`).
    pub recipient: String,
}

/// What sealing a backup needs: its key and the passphrase for the archive.
pub struct BackupKeys {
    identity: x25519::Identity,
    passphrase: SecretString,
}

/// Holds the keys in [`crate::job::JobOptions`] without ever saving them.
#[derive(Clone, Default)]
pub struct KeySlot(pub Option<Arc<BackupKeys>>);

impl std::fmt::Debug for KeySlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "KeySlot(set)"
        } else {
            "KeySlot(empty)"
        })
    }
}

impl PartialEq for KeySlot {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for KeySlot {}

impl EncryptedBackupSettings {
    /// Starts a new backup in `dir`, protected by `passphrase`.
    pub fn create(dir: &Path, passphrase: SecretString) -> io::Result<(Self, BackupKeys)> {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let parts = dir.join(format!("erasecord-backup-{stamp}.parts"));
        let archive = dir.join(format!("erasecord-backup-{stamp}.tar.age"));
        std::fs::create_dir_all(&parts)?;
        let identity = x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        write_encrypted(
            &parts.join(KEY_FILE),
            &passphrase,
            identity.to_string().expose_secret().as_bytes(),
        )?;
        std::fs::write(parts.join("README.txt"), PARTS_README)?;
        Ok((
            EncryptedBackupSettings {
                parts,
                archive,
                recipient,
            },
            BackupKeys {
                identity,
                passphrase,
            },
        ))
    }

    /// The keys of an existing backup, from its passphrase; for continuing
    /// and sealing it.
    pub fn unlock(&self, passphrase: SecretString) -> Result<BackupKeys, String> {
        let identity = read_key(&self.parts, &passphrase)?;
        if identity.to_public().to_string() != self.recipient {
            return Err("this key does not belong to the backup".into());
        }
        Ok(BackupKeys {
            identity,
            passphrase,
        })
    }
}

fn read_key(parts: &Path, passphrase: &SecretString) -> Result<x25519::Identity, String> {
    let file = File::open(parts.join(KEY_FILE))
        .map_err(|err| format!("cannot read the backup's key: {err}"))?;
    let mut text = String::new();
    decrypt(passphrase, BufReader::new(file))?
        .read_to_string(&mut text)
        .map_err(|err| format!("cannot read the backup's key: {err}"))?;
    x25519::Identity::from_str(text.trim()).map_err(|err| err.to_string())
}

type Sealer = StreamWriter<BufWriter<File>>;

struct OpenPart {
    tar: tar::Builder<Sealer>,
    partial: PathBuf,
    path: PathBuf,
    messages: usize,
    bytes: u64,
}

struct State {
    part: Option<OpenPart>,
    next_part: u32,
    records: Option<ExportWriter<Sealer>>,
    records_error: Option<String>,
}

/// The backup being written by a run.
pub struct EncryptedBackup {
    settings: EncryptedBackupSettings,
    recipient: x25519::Recipient,
    keys: Option<Arc<BackupKeys>>,
    downloader: Backup,
    state: Mutex<State>,
}

/// A finished backup.
#[derive(Debug, Clone)]
pub struct Sealed {
    pub archive: PathBuf,
    pub files: u64,
    pub messages: u64,
}

impl EncryptedBackup {
    /// Opens the backup for writing. Without `keys` it can still be added
    /// to, but not sealed.
    pub fn open(
        settings: &EncryptedBackupSettings,
        keys: Option<Arc<BackupKeys>>,
    ) -> io::Result<Self> {
        std::fs::create_dir_all(&settings.parts)?;
        let recipient =
            x25519::Recipient::from_str(&settings.recipient).map_err(io::Error::other)?;
        let next_part = numbered(&settings.parts, "part-")?
            .last()
            .map_or(1, |(n, _)| n + 1);
        let next_records = numbered(&settings.parts, "records-")?
            .last()
            .map_or(1, |(n, _)| n + 1);
        let file = File::create(
            settings
                .parts
                .join(format!("records-{next_records:05}.jsonl.age")),
        )?;
        let sealer =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .map_err(io::Error::other)?
                .wrap_output(BufWriter::new(file))?;
        let records = ExportWriter::new(sealer, ExportFormat::JsonLines)?;
        Ok(EncryptedBackup {
            settings: settings.clone(),
            recipient,
            keys,
            downloader: Backup::downloader(&settings.parts)?,
            state: Mutex::new(State {
                part: None,
                next_part,
                records: Some(records),
                records_error: None,
            }),
        })
    }

    /// Waits between downloads; see [`Backup::set_pace`].
    pub(crate) fn set_pace(&mut self, pace: Arc<crate::pace::Pace>) {
        self.downloader.set_pace(pace);
    }

    pub fn settings(&self) -> &EncryptedBackupSettings {
        &self.settings
    }

    /// Downloads the attachments of `message` and adds them to the open
    /// part. Returns their paths in the archive. On any failure nothing is
    /// added, and the message must be kept.
    pub async fn add(&self, client: &Client, message: &Message) -> Result<Vec<String>, String> {
        let mut files = Vec::with_capacity(message.attachments.len());
        for (index, attachment) in message.attachments.iter().enumerate() {
            let (url, name, relative) = attachment_path(message, index, attachment)?;
            let data = self
                .downloader
                .download_bytes(client, &url)
                .await
                .map_err(|err| format!("could not save {name}: {err}"))?;
            files.push((relative, data));
        }
        let mut state = self.state.lock().unwrap();
        let part = match &mut state.part {
            Some(part) => part,
            None => {
                let number = state.next_part;
                state.next_part += 1;
                state.part = Some(self.new_part(number).map_err(|err| err.to_string())?);
                state.part.as_mut().expect("just opened")
            }
        };
        let mtime = message.id.created_at().timestamp().max(0) as u64;
        let mut saved = Vec::with_capacity(files.len());
        for (relative, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(mtime);
            part.tar
                .append_data(&mut header, &relative, data.as_slice())
                .map_err(|err| format!("could not write the backup: {err}"))?;
            part.bytes += data.len() as u64;
            saved.push(relative);
        }
        part.messages += 1;
        Ok(saved)
    }

    fn new_part(&self, number: u32) -> io::Result<OpenPart> {
        let path = self
            .settings
            .parts
            .join(format!("part-{number:05}.tar.age"));
        let partial = path.with_extension("partial");
        let file = File::create(&partial)?;
        let sealer = age::Encryptor::with_recipients(std::iter::once(
            &self.recipient as &dyn age::Recipient,
        ))
        .map_err(io::Error::other)?
        .wrap_output(BufWriter::new(file))?;
        Ok(OpenPart {
            tar: tar::Builder::new(sealer),
            partial,
            path,
            messages: 0,
            bytes: 0,
        })
    }

    /// Whether the open part should be sealed before more is added.
    pub fn part_is_full(&self) -> bool {
        let state = self.state.lock().unwrap();
        state
            .part
            .as_ref()
            .is_some_and(|p| p.messages >= PART_MESSAGES || p.bytes >= PART_BYTES)
    }

    /// Seals the open part and makes sure it is on disk. Messages whose
    /// files are in it may be deleted after this.
    pub fn commit(&self) -> io::Result<()> {
        let part = self.state.lock().unwrap().part.take();
        let Some(part) = part else {
            return Ok(());
        };
        let sealer = part.tar.into_inner()?;
        let file = sealer.finish()?.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        std::fs::rename(&part.partial, &part.path)?;
        Ok(())
    }

    /// Records deleted messages, those of others whose files were saved
    /// (and the names of their servers and DMs).
    pub fn observe(&self, event: &Event) {
        if !matches!(
            event,
            Event::TargetStarted { .. } | Event::Deleted { .. } | Event::SavedFromOthers { .. }
        ) {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if let Some(records) = &mut state.records {
            if let Err(err) = records.observe(event) {
                state.records_error = Some(err.to_string());
                state.records = None;
            }
        }
    }

    /// Commits and closes everything written so far.
    fn close(&self) -> Result<(), String> {
        self.commit().map_err(|err| err.to_string())?;
        let mut state = self.state.lock().unwrap();
        if let Some(records) = state.records.take() {
            let sealer = records.finish().map_err(|err| err.to_string())?;
            let file = sealer
                .finish()
                .and_then(|w| w.into_inner().map_err(|e| e.into_error()))
                .map_err(|err| err.to_string())?;
            file.sync_all().map_err(|err| err.to_string())?;
        }
        match state.records_error.take() {
            Some(err) => Err(format!("the list of messages could not be written: {err}")),
            None => Ok(()),
        }
    }

    /// Ends the run's part of the backup without combining it, e.g. when
    /// the run was stopped; a continued run adds to it.
    pub fn suspend(self) -> Result<PathBuf, String> {
        self.close()?;
        Ok(self.settings.parts.clone())
    }

    /// Combines all parts into the archive, checks it and removes the parts.
    pub fn seal(self) -> Result<Sealed, String> {
        self.close()?;
        let keys = self
            .keys
            .clone()
            .ok_or("the passphrase is needed to finish the backup")?;
        seal_parts(&self.settings, &keys)
    }
}

/// Files in `dir` named `<prefix><number>…`, sorted by number.
fn numbered(dir: &Path, prefix: &str) -> io::Result<Vec<(u32, PathBuf)>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".age") {
            continue;
        }
        if let Some(rest) = name.strip_prefix(prefix) {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse() {
                found.push((number, path));
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The messages of all record files, oldest first; a file cut off by a
/// crash gives what it has.
fn read_records(
    parts: &Path,
    identity: &x25519::Identity,
) -> Result<Vec<serde_json::Value>, String> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for (_, path) in numbered(parts, "records-").map_err(|err| err.to_string())? {
        let file = File::open(&path).map_err(|err| err.to_string())?;
        let Ok(reader) = decrypt_with(identity, BufReader::new(file)) else {
            continue;
        };
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            if let Ok(row) = serde_json::from_str::<serde_json::Value>(&line) {
                let id = row["message_id"].as_str().unwrap_or_default().to_owned();
                if seen.insert(id) {
                    rows.push(row);
                }
            }
        }
    }
    rows.sort_by(|a, b| a["sent_at"].as_str().cmp(&b["sent_at"].as_str()));
    Ok(rows)
}

fn seal_parts(settings: &EncryptedBackupSettings, keys: &BackupKeys) -> Result<Sealed, String> {
    let io = |err: io::Error| err.to_string();
    let parts = numbered(&settings.parts, "part-").map_err(io)?;
    let rows = read_records(&settings.parts, &keys.identity)?;

    let partial = settings.archive.with_extension("partial");
    let file = File::create(&partial).map_err(io)?;
    let mut tar = tar::Builder::new(encrypt(&keys.passphrase, BufWriter::new(file)).map_err(io)?);
    let mut names = HashSet::new();
    let mut files = 0;
    let written: Result<(), String> = (|| {
        for (_, path) in &parts {
            let file = File::open(path).map_err(io)?;
            let reader = decrypt_with(&keys.identity, BufReader::new(file))?;
            let mut archive = tar::Archive::new(reader);
            for entry in archive.entries().map_err(io)? {
                let mut entry = entry.map_err(io)?;
                let name = entry.path().map_err(io)?.to_string_lossy().into_owned();
                // A continued run may have saved a file again.
                if !names.insert(name.clone()) {
                    continue;
                }
                let mut header = entry.header().clone();
                tar.append_data(&mut header, &name, &mut entry)
                    .map_err(io)?;
                files += 1;
            }
        }
        let list = serde_json::to_vec_pretty(&rows).map_err(|err| err.to_string())?;
        for (name, data) in [
            ("messages.json", list.as_slice()),
            ("README.txt", ARCHIVE_README.as_bytes()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(chrono::Utc::now().timestamp().max(0) as u64);
            tar.append_data(&mut header, name, data).map_err(io)?;
        }
        let file = tar
            .into_inner()
            .and_then(|sealer| sealer.finish())
            .and_then(|w| w.into_inner().map_err(|e| e.into_error()))
            .map_err(io)?;
        file.sync_all().map_err(io)?;
        Ok(())
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("could not write the backup archive: {err}"));
    }

    // Read it back before anything is removed.
    let check = (|| -> Result<u64, String> {
        let file = File::open(&partial).map_err(io)?;
        let mut archive = tar::Archive::new(decrypt(&keys.passphrase, BufReader::new(file))?);
        let mut count = 0;
        for entry in archive.entries().map_err(io)? {
            let mut entry = entry.map_err(io)?;
            io::copy(&mut entry, &mut io::sink()).map_err(io)?;
            count += 1;
        }
        Ok(count)
    })();
    match check {
        Ok(count) if count == files + 2 => {}
        Ok(count) => {
            let _ = std::fs::remove_file(&partial);
            return Err(format!(
                "the backup archive is incomplete ({count} of {} entries)",
                files + 2
            ));
        }
        Err(err) => {
            let _ = std::fs::remove_file(&partial);
            return Err(format!("the backup archive could not be read back: {err}"));
        }
    }
    std::fs::rename(&partial, &settings.archive).map_err(io)?;
    std::fs::remove_dir_all(&settings.parts).map_err(io)?;
    Ok(Sealed {
        archive: settings.archive.clone(),
        files,
        messages: rows.len() as u64,
    })
}

/// What [`open`] unpacked.
#[derive(Debug, Clone, Serialize)]
pub struct Opened {
    pub folder: PathBuf,
    pub files: u64,
    pub messages: u64,
}

/// Decrypts and unpacks an EraseCord backup (the `.tar.age` archive or a
/// `.parts` folder) or an encrypted export into `into`.
pub fn open(path: &Path, passphrase: &SecretString, into: &Path) -> Result<Opened, String> {
    let io = |err: io::Error| err.to_string();
    std::fs::create_dir_all(into).map_err(io)?;
    let mut opened = Opened {
        folder: into.to_owned(),
        files: 0,
        messages: 0,
    };
    if path.is_dir() {
        let identity = read_key(path, passphrase)?;
        for (_, part) in numbered(path, "part-").map_err(io)? {
            let file = File::open(&part).map_err(io)?;
            let mut archive = tar::Archive::new(decrypt_with(&identity, BufReader::new(file))?);
            opened.files += unpack(&mut archive, into)?;
        }
        let rows = read_records(path, &identity)?;
        opened.messages = rows.len() as u64;
        let list = serde_json::to_vec_pretty(&rows).map_err(|err| err.to_string())?;
        std::fs::write(into.join("messages.json"), list).map_err(io)?;
        return Ok(opened);
    }

    let file = File::open(path).map_err(io)?;
    let mut reader = decrypt(passphrase, BufReader::new(file))?;
    // A tar archive says "ustar" at byte 257 of its first block.
    let mut head = Vec::with_capacity(512);
    (&mut reader).take(512).read_to_end(&mut head).map_err(io)?;
    let is_tar = head.len() == 512 && &head[257..262] == b"ustar";
    let mut whole = io::Cursor::new(head).chain(reader);
    if is_tar {
        let mut archive = tar::Archive::new(whole);
        opened.files = unpack(&mut archive, into)?;
        if let Ok(list) = std::fs::read(into.join("messages.json")) {
            opened.messages = serde_json::from_slice::<Vec<serde_json::Value>>(&list)
                .map_or(0, |r| r.len() as u64);
            opened.files = opened.files.saturating_sub(2);
        }
    } else {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.strip_suffix(".age").unwrap_or(n).to_owned())
            .unwrap_or_else(|| "decrypted".into());
        let target = into.join(name);
        let mut out = BufWriter::new(File::create(&target).map_err(io)?);
        io::copy(&mut whole, &mut out).map_err(io)?;
        out.flush().map_err(io)?;
        opened.files = 1;
    }
    Ok(opened)
}

/// Unpacks into `into`, refusing paths that would leave it.
fn unpack<R: Read>(archive: &mut tar::Archive<R>, into: &Path) -> Result<u64, String> {
    let mut count = 0;
    for entry in archive.entries().map_err(|err| err.to_string())? {
        let mut entry = entry.map_err(|err| err.to_string())?;
        if entry.unpack_in(into).map_err(|err| err.to_string())? {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrases_are_twelve_words_from_the_list() {
        let words: HashSet<&str> = WORDS.lines().collect();
        assert_eq!(words.len(), 2048);
        let a = generate_passphrase();
        let b = generate_passphrase();
        assert_ne!(a, b);
        assert_eq!(a.split(' ').count(), PASSPHRASE_WORDS);
        assert!(a.split(' ').all(|w| words.contains(w)));
    }

    #[test]
    fn a_wrong_passphrase_is_refused() {
        let mut data = Vec::new();
        let mut writer = encrypt(&secret("right"), &mut data).unwrap();
        writer.write_all(b"hello").unwrap();
        writer.finish().unwrap();
        let mut text = String::new();
        decrypt(&secret("right"), data.as_slice())
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "hello");
        let wrong = decrypt(&secret("wrong"), data.as_slice()).err().unwrap();
        assert!(wrong.contains("wrong passphrase"), "{wrong}");
    }

    #[test]
    fn unlocking_needs_the_right_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let (settings, _) = EncryptedBackupSettings::create(dir.path(), secret("pass")).unwrap();
        assert!(settings.unlock(secret("pass")).is_ok());
        assert!(settings.unlock(secret("nope")).is_err());
        // The folder holds nothing readable without it.
        let key = std::fs::read(settings.parts.join(KEY_FILE)).unwrap();
        assert!(!String::from_utf8_lossy(&key).contains("AGE-SECRET-KEY"));
    }
}
