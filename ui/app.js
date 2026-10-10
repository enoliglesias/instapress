const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);

function show(element, text, isError = false) {
  element.className = isError ? "error" : "";
  element.textContent = text;
}

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

const TYPE_LABEL = { IMAGE: "Photo", VIDEO: "Reel or video", CAROUSEL_ALBUM: "Album", UNKNOWN: "Post" };

async function refreshTokenStatus() {
  try {
    const saved = await invoke("has_token");
    show($("token-status"), saved ? "A token is saved in this computer's credential store." : "No token saved yet.");
  } catch (message) {
    show($("token-status"), String(message), true);
  }
}

function renderPosts(posts) {
  const list = $("posts");
  list.replaceChildren();
  for (const post of posts) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "post";

    const thumb = document.createElement("img");
    thumb.alt = "";
    thumb.width = 72;
    thumb.height = 72;
    // The CDN does not need to learn which app is asking, and off-screen thumbnails can wait.
    thumb.referrerPolicy = "no-referrer";
    thumb.loading = "lazy";
    if (post.preview_url) thumb.src = post.preview_url;

    const text = document.createElement("span");
    const meta = document.createElement("small");
    const posted = post.timestamp ? new Date(post.timestamp) : null;
    const date = posted && !Number.isNaN(posted.getTime()) ? posted.toLocaleDateString() : "";
    meta.textContent = [TYPE_LABEL[post.media_type] ?? "Post", date].filter(Boolean).join(" · ");
    const caption = document.createElement("span");
    caption.className = "excerpt";
    caption.textContent = post.caption ? post.caption.slice(0, 140) : "(no caption)";
    text.append(meta, caption);

    button.append(thumb, text);
    button.addEventListener("click", () => {
      $("caption").textContent = post.caption ?? "";
      $("caption-panel").hidden = false;
    });
    item.append(button);
    list.append(item);
  }
}

// Only the newest request may update the list, so a slow earlier answer cannot overwrite it.
let loadSeq = 0;

async function loadPosts() {
  const seq = ++loadSeq;
  $("posts-refresh").disabled = true;
  show($("posts-message"), "Loading...");
  $("caption-panel").hidden = true;
  try {
    const posts = await invoke("list_posts");
    if (seq !== loadSeq) return;
    renderPosts(posts);
    show($("posts-message"), posts.length ? "" : "No posts found.");
  } catch (message) {
    if (seq !== loadSeq) return;
    renderPosts([]);
    show($("posts-message"), String(message), true);
  } finally {
    if (seq === loadSeq) $("posts-refresh").disabled = false;
  }
}

$("token-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const token = $("token").value;
  // The secret never stays in the page, whether or not the save works.
  $("token").value = "";
  try {
    await invoke("save_token", { token });
    show($("token-message"), "Token saved.");
    await refreshTokenStatus();
    await loadPosts();
  } catch (message) {
    show($("token-message"), String(message), true);
  }
});

$("token-clear").addEventListener("click", async () => {
  try {
    await invoke("clear_token");
    loadSeq += 1;
    $("posts-refresh").disabled = false;
    show($("token-message"), "Token removed.");
    renderPosts([]);
    await refreshTokenStatus();
  } catch (message) {
    show($("token-message"), String(message), true);
  }
});

$("posts-refresh").addEventListener("click", loadPosts);

$("source-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const target = await invoke("classify_instagram_url", { url: $("url").value });
    show($("result"), describe(target));
  } catch (message) {
    show($("result"), String(message), true);
  }
});

refreshTokenStatus().then(async () => {
  if (await invoke("has_token").catch(() => false)) await loadPosts();
});
