const { invoke } = window.__TAURI__.core;

const form = document.getElementById("source-form");
const input = document.getElementById("url");
const result = document.getElementById("result");

function describe(target) {
  switch (target.kind) {
    case "profile":
      return `Profile @${target.username}`;
    case "post":
      return `Post ${target.shortcode}`;
    case "reel":
      return `Reel ${target.shortcode}`;
    default:
      return "Unknown target";
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const target = await invoke("classify_instagram_url", { url: input.value });
    result.className = "";
    result.textContent = describe(target);
  } catch (message) {
    result.className = "error";
    result.textContent = String(message);
  }
});
