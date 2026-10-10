const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);

const MAX_IMAGE_BYTES = 10 * 1024 * 1024;
const TYPE_LABEL = { IMAGE: "Photo", VIDEO: "Reel or video", CAROUSEL_ALBUM: "Album", UNKNOWN: "Post" };

// Errors are announced at once (`alert`), everything else politely (`status`).
function show(element, text, isError = false) {
  element.className = isError ? "error" : "";
  element.setAttribute("role", isError ? "alert" : "status");
  element.textContent = text;
}

function formatSize(bytes) {
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
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

// --- Instagram access ---

const DAY_MS = 24 * 60 * 60 * 1000;

function describeExpiry(expiresAt) {
  if (!expiresAt) return "Its expiry date is not known yet.";
  const date = new Date(expiresAt * 1000);
  const days = Math.ceil((date.getTime() - Date.now()) / DAY_MS);
  if (days <= 0) return `It expired on ${date.toLocaleDateString()}: generate a new one in the Meta panel.`;
  return `It expires on ${date.toLocaleDateString()} (in ${days} ${days === 1 ? "day" : "days"}).`;
}

async function refreshTokenStatus() {
  try {
    const status = await invoke("token_status");
    show(
      $("token-status"),
      status.has_token
        ? `A token is saved in this computer's credential store. ${describeExpiry(status.expires_at)}`
        : "No token saved yet.",
    );
    $("token-renew").disabled = !status.has_token;
    return status.has_token;
  } catch (message) {
    show($("token-status"), String(message), true);
    return false;
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

$("token-renew").addEventListener("click", async () => {
  $("token-renew").disabled = true;
  show($("token-message"), "Renewing the token...");
  try {
    await invoke("refresh_token");
    show($("token-message"), "The token was renewed for about 60 days.");
  } catch (message) {
    show($("token-message"), `Could not renew the token: ${message}`, true);
  }
  await refreshTokenStatus();
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

// --- WordPress access ---

async function refreshWordPressSettings() {
  try {
    const settings = await invoke("get_wordpress_settings");
    if (settings.site) $("wp-site").value = settings.site;
    if (settings.username) $("wp-user").value = settings.username;
    show(
      $("wp-status"),
      settings.has_password ? "Settings are saved in this computer's credential store." : "No WordPress settings saved yet.",
    );
  } catch (message) {
    show($("wp-status"), String(message), true);
  }
}

$("wp-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const appPassword = $("wp-password").value;
  $("wp-password").value = "";
  try {
    await invoke("save_wordpress_settings", {
      site: $("wp-site").value,
      username: $("wp-user").value,
      appPassword,
    });
    show($("wp-message"), "WordPress settings saved.");
    await refreshWordPressSettings();
  } catch (message) {
    show($("wp-message"), String(message), true);
  }
});

$("wp-clear").addEventListener("click", async () => {
  try {
    await invoke("clear_wordpress_settings");
    $("wp-site").value = "";
    $("wp-user").value = "";
    show($("wp-message"), "WordPress settings removed.");
    await refreshWordPressSettings();
  } catch (message) {
    show($("wp-message"), String(message), true);
  }
});

// --- Recent posts ---

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
      for (const other of list.querySelectorAll("button.post")) other.removeAttribute("aria-current");
      button.setAttribute("aria-current", "true");
      openDraft(post);
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

$("posts-refresh").addEventListener("click", loadPosts);

// --- Draft ---

let postImageUrl = null;
let postPermalink = null;
let chosenFile = null;
let slugEdited = false;
let slugTimer = null;
// The categories of the site, loaded the first time a draft is opened.
let categories = null;
const selectedCategories = new Set();

async function openDraft(post) {
  try {
    const { title, html, slug, meta_description: metaDescription } = await invoke("prepare_draft", {
      caption: post.caption ?? "",
    });
    $("draft-title-input").value = title;
    // The slug follows the title until the user edits it.
    $("draft-slug-input").value = slug;
    slugEdited = false;
    $("draft-meta").value = metaDescription;
    $("draft-keyphrase").value = "";
    updateMetaCount();
    selectedCategories.clear();
    $("category-search").value = "";
    if (categories === null) loadCategories();
    else renderCategories();
    // `html` is built by the backend with every special character of the caption escaped.
    $("editor").innerHTML = html;
  } catch (message) {
    show($("publish-message"), String(message), true);
    return;
  }

  postImageUrl = post.preview_url ?? null;
  const preview = $("image-post-preview");
  preview.replaceChildren();
  if (postImageUrl) {
    const picture = document.createElement("img");
    picture.alt = "";
    picture.width = 96;
    picture.height = 96;
    picture.referrerPolicy = "no-referrer";
    picture.src = postImageUrl;
    preview.append(picture);
  }
  preview.hidden = !postImageUrl;
  $("image-post").disabled = !postImageUrl;
  (postImageUrl ? $("image-post") : $("image-none")).checked = true;

  // The player is offered only for a video post, and it is on by default there.
  postPermalink = post.media_type === "VIDEO" && post.permalink ? post.permalink : null;
  $("video-options").hidden = !postPermalink;
  $("video-embed").checked = Boolean(postPermalink);

  chosenFile = null;
  $("image-input").value = "";
  show($("image-file-info"), "");
  $("publish-result").hidden = true;
  $("publish-warning").hidden = true;
  show($("publish-message"), "");
  $("draft").hidden = false;
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  $("draft").scrollIntoView({ behavior: reduceMotion ? "auto" : "smooth", block: "start" });
  // Keyboard and screen reader users get a cue that the section appeared.
  $("draft-title-input").focus({ preventScroll: true });
}

for (const button of document.querySelectorAll(".toolbar button")) {
  // Keep the text selection in the editor when a toolbar button is pressed.
  button.addEventListener("mousedown", (event) => event.preventDefault());
  button.addEventListener("click", () => {
    const command = button.dataset.command;
    if (command === "heading") {
      // A second press on a heading turns it back into a paragraph.
      const current = String(document.queryCommandValue("formatBlock")).toLowerCase();
      document.execCommand("formatBlock", false, current === "h2" ? "p" : "h2");
    } else {
      document.execCommand(command);
    }
    $("editor").focus();
  });
}

// Pasted text goes in as plain text: the formatting of another page is not wanted, and the
// backend would strip most of it anyway.
$("editor").addEventListener("paste", (event) => {
  event.preventDefault();
  const text = event.clipboardData?.getData("text/plain") ?? "";
  if (text) document.execCommand("insertText", false, text);
});

$("image-input").addEventListener("change", () => {
  const file = $("image-input").files[0];
  chosenFile = null;
  show($("image-file-info"), "");
  if (!file) return;
  if (file.size > MAX_IMAGE_BYTES) {
    $("image-input").value = "";
    show($("image-file-info"), "That image is larger than 10 MB.", true);
    return;
  }
  const reader = new FileReader();
  reader.addEventListener("load", () => {
    chosenFile = { dataUrl: reader.result, name: file.name };
    $("image-file").checked = true;
    show($("image-file-info"), `${file.name} (${formatSize(file.size)})`);
  });
  reader.addEventListener("error", () => show($("image-file-info"), "That file could not be read.", true));
  reader.readAsDataURL(file);
});

function imageChoice() {
  if ($("image-post").checked && postImageUrl) return { kind: "post_image", url: postImageUrl };
  if ($("image-file").checked && chosenFile) {
    return { kind: "file", data_url: chosenFile.dataUrl, filename: chosenFile.name };
  }
  return { kind: "none" };
}

function videoChoice() {
  if (postPermalink && $("video-embed").checked) {
    return { kind: "instagram_embed", permalink: postPermalink };
  }
  return { kind: "none" };
}

// Compare without case or accents, so "tartas" finds "Tartas" and "postres" finds "Póstres".
function foldText(text) {
  return text.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
}

function renderCategories() {
  const list = $("category-list");
  list.replaceChildren();
  const query = foldText($("category-search").value.trim());
  for (const category of categories ?? []) {
    if (query && !foldText(category.name).includes(query)) continue;
    const item = document.createElement("li");
    item.dataset.depth = String(Math.min(category.depth, 5));
    const label = document.createElement("label");
    label.className = "choice";
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = selectedCategories.has(category.id);
    box.addEventListener("change", () => {
      if (box.checked) selectedCategories.add(category.id);
      else selectedCategories.delete(category.id);
    });
    const name = document.createElement("span");
    name.textContent = category.name;
    label.append(box, name);
    item.append(label);
    list.append(item);
  }
}

async function loadCategories() {
  show($("category-message"), "Loading categories...");
  try {
    categories = await invoke("list_categories");
    show($("category-message"), categories.length ? "" : "The site has no categories.");
  } catch (message) {
    categories = null;
    show($("category-message"), `Could not load the categories: ${message}`, true);
  }
  renderCategories();
}

$("category-search").addEventListener("input", renderCategories);
$("categories-reload").addEventListener("click", loadCategories);

function updateMetaCount() {
  const length = $("draft-meta").value.length;
  $("draft-meta-count").textContent = length
    ? `${length} characters. Search engines show about 155.`
    : "Empty: no meta description is sent.";
}

// Keep the slug in step with the title until the user writes their own.
$("draft-title-input").addEventListener("input", () => {
  if (slugEdited) return;
  clearTimeout(slugTimer);
  slugTimer = setTimeout(async () => {
    try {
      $("draft-slug-input").value = await invoke("suggest_slug", { title: $("draft-title-input").value });
    } catch (message) {
      show($("publish-message"), String(message), true);
    }
  }, 150);
});
$("draft-slug-input").addEventListener("input", () => {
  slugEdited = true;
});
$("draft-meta").addEventListener("input", updateMetaCount);

$("publish").addEventListener("click", async () => {
  if ($("image-file").checked && !chosenFile) {
    show($("publish-message"), "Choose an image file or pick another image option.", true);
    return;
  }
  $("publish").disabled = true;
  $("publish-result").hidden = true;
  $("publish-warning").hidden = true;
  show($("publish-message"), "Creating the draft...");
  try {
    const post = await invoke("create_wordpress_draft", {
      draft: {
        title: $("draft-title-input").value,
        slug: $("draft-slug-input").value,
        meta_description: $("draft-meta").value,
        focus_keyphrase: $("draft-keyphrase").value,
        categories: [...selectedCategories],
        html: $("editor").innerHTML,
        image: imageChoice(),
        video: videoChoice(),
      },
    });
    show($("publish-message"), "Draft created in WordPress.");
    $("publish-url").value = post.edit_url;
    $("publish-result").hidden = false;
    // `false` means WordPress ignored the field: the site has to expose Yoast's meta to the API.
    const unsaved = [];
    if (post.meta_description_saved === false) unsaved.push("meta description");
    if (post.focus_keyphrase_saved === false) unsaved.push("focus keyphrase");
    $("publish-warning").hidden = unsaved.length === 0;
    $("publish-warning").textContent =
      `WordPress did not save the ${unsaved.join(" and the ")}: the site has to expose Yoast's fields to the REST API. See knowledge/operational/yoast-meta-description.md.`;
  } catch (message) {
    // An answer that never arrived does not prove nothing was created.
    const unsure = String(message).includes("did not complete")
      ? " The draft may still have been created: check the drafts in WordPress before trying again."
      : "";
    show($("publish-message"), `${message}${unsure}`, true);
  } finally {
    $("publish").disabled = false;
  }
});

$("publish-copy").addEventListener("click", async () => {
  const field = $("publish-url");
  try {
    await navigator.clipboard.writeText(field.value);
    show($("publish-message"), "Address copied.");
  } catch {
    field.select();
    show($("publish-message"), "Press Ctrl+C (or Cmd+C) to copy the selected address.");
  }
});

// --- Check a URL ---

$("source-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const target = await invoke("classify_instagram_url", { url: $("url").value });
    show($("result"), describe(target));
  } catch (message) {
    show($("result"), String(message), true);
  }
});

// --- Start ---

async function start() {
  refreshWordPressSettings();
  // Renew the token when it is about to expire. A failure must not stop the app: the old token
  // may still work, and the message says what happened.
  try {
    const outcome = await invoke("renew_token_if_due");
    if (outcome.renewed) show($("token-message"), "The token was renewed for about 60 days.");
  } catch (message) {
    show($("token-message"), `Could not renew the token: ${message}`, true);
  }
  if (await refreshTokenStatus()) await loadPosts();
}

start();
