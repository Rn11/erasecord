//! Words too common to say anything about someone, in the languages of the
//! app. People mix languages in chat, so all lists apply at once.

use std::collections::HashSet;
use std::sync::LazyLock;

const ENGLISH: &str = "a about above after again against all am an and any are aren't as at be because been \
before being below between both but by can can't cannot could couldn't did didn't do does doesn't doing don't \
down during each few for from further had hadn't has hasn't have haven't having he he'd he'll he's her here \
here's hers herself him himself his how how's i i'd i'll i'm i've if in into is isn't it it's its itself let's \
me more most mustn't my myself no nor not of off on once only or other ought our ours ourselves out over own \
same shan't she she'd she'll she's should shouldn't so some such than that that's the their theirs them \
themselves then there there's these they they'd they'll they're they've this those through to too under until \
up very was wasn't we we'd we'll we're we've were weren't what what's when when's where where's which while who \
who's whom why why's with won't would wouldn't you you'd you'll you're you've your yours yourself yourselves \
im ive dont didnt doesnt isnt wasnt cant wont thats whats theres youre also just like get got will one oh ok \
okay yeah yes yep nah no lol u ur r gonna wanna gotta really would could should much many even still well";

const GERMAN: &str = "aber alle allem allen aller alles als also am an ander andere anderem anderen anderer \
anderes anderm andern anderr anders auch auf aus bei bin bis bist da damit dann das dass daß dein deine deinem \
deinen deiner deines dem den denn der derer des dessen dich dir du dies diese diesem diesen dieser dieses doch \
dort durch ein eine einem einen einer eines einig einige einigem einigen einiger einiges einmal er es etwas \
euch euer eure eurem euren eurer eures für gegen gewesen hab habe haben hat hatte hatten hier hin hinter ich \
ihm ihn ihnen ihr ihre ihrem ihren ihrer ihres im in indem ins ist ja jede jedem jeden jeder jedes jene jenem \
jenen jener jenes jetzt kann kein keine keinem keinen keiner keines können könnte machen man manche manchem \
manchen mancher manches mein meine meinem meinen meiner meines mich mir mit muss musste nach nicht nichts noch \
nun nur ob oder ohne sehr sein seine seinem seinen seiner seines selbst sich sie sind so solche solchem \
solchen solcher solches soll sollte sondern sonst über um und uns unsere unserem unseren unser unseres unter \
viel vom von vor während war waren warst was weg weil weiter welche welchem welchen welcher welches wenn werde \
werden wie wieder will wir wird wirst wo wollen wollte würde würden zu zum zur zwar zwischen halt mal schon \
eh ne nee jo joa gut okay ach";

const SPANISH: &str = "a al algo algunas algunos ante antes como con contra cual cuando de del desde donde \
durante e el él ella ellas ellos en entre era erais eran eras eres es esa esas ese eso esos esta está están \
estar estas este esto estos estoy fue fueron fui ha había han has hasta hay he la las le les lo los más me mi \
mí mis mucho muchos muy nada ni no nos nosotros o os otra otras otro otros para pero poco por porque que qué \
quien se sea ser si sí sin sobre son su sus también te tengo ti tiene tienen todo todos tu tú tus un una uno \
unos vosotros y ya yo pues bueno vale jaja jajaja xd";

const SWEDISH: &str = "alla allt att av blev bli blir blivit de dem den denna deras dess dessa det detta dig \
din dina ditt du där då efter ej eller en er era ert ett från för ha hade han hans har henne hennes hon honom \
hur här i icke ingen inom inte jag ju kan kunde man med mellan men mig min mina mitt mot mycket ni nu när något \
några och om oss på samma sedan sig sin sina sitt själv skulle som så sådan sådana sådant till under upp ut \
utan vad var vara varför varit varje vars vart vem vi vid vilka vilkas vilken vilket vår våra vårt än är åt \
över ja nej okej typ bara också";

const UKRAINIAN: &str = "а або аж але б би був була були було бути в вам вас вже ви від він вона вони воно все \
всі всього втім де до для же з за й і із їй їм їх його її коли кому куди лише ми мене мені мій міг на навіть \
нам нас не ней ні ніж ну о об од от по при про саме свій себе так такий там те теж тим то тобі тоді того той \
ту тут ти у хоч це цей ці чи чого що щоб я як яка який які так ага ок";

pub(crate) static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [ENGLISH, GERMAN, SPANISH, SWEDISH, UKRAINIAN]
        .iter()
        .flat_map(|list| list.split_whitespace())
        .collect()
});

pub(crate) fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(word)
}
