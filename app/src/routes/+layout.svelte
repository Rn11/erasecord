<script lang="ts">
  import "../app.css";
  import "../emoji.css";

  // WebView2 on Windows draws the bundled colour emoji font (see app.css).
  if (typeof navigator !== "undefined" && navigator.userAgent.includes("Windows")) {
    document.documentElement.classList.add("windows");
  }

  let { children } = $props();
</script>

{@render children()}
