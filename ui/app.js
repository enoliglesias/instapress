const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);

const MAX_IMAGE_BYTES = 10 * 1024 * 1024;
const TYPE_LABEL = { IMAGE: "Photo", VIDEO: "Reel or video", CAROUSEL_ALBUM: "Album", UNKNOWN: "Post" };

// Errors are announced at once (`alert`), everything else politely (`status`).
function show(element, text, isError = false) {
  element.classList.toggle("error", isError);
  element.setAttribute("role", isError ? "alert" : "status");
  element.textContent = text;
}

function formatSize(bytes) {
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

// --- Layout ---
// The View > 2 columns item of the menu chooses between one column and two. The choice is
// remembered by the window and the menu is told on start, so its check mark agrees.

const LAYOUT_KEY = "instapress-layout";

function applyLayout(twoColumns) {
  document.documentElement.dataset.layout = twoColumns ? "two" : "one";
}

function savedLayout() {
  try {
    return localStorage.getItem(LAYOUT_KEY) === "two";
  } catch {
    // Storage may be unavailable; the window then opens in one column each time.
    return false;
  }
}

function rememberLayout(twoColumns) {
  try {
    localStorage.setItem(LAYOUT_KEY, twoColumns ? "two" : "one");
  } catch {
    // Not remembering the choice is harmless.
  }
}

applyLayout(savedLayout());
invoke("set_layout_menu", { twoColumns: savedLayout() }).catch(() => {});
window.__TAURI__.event.listen("layout", (event) => {
  const twoColumns = Boolean(event.payload);
  applyLayout(twoColumns);
  rememberLayout(twoColumns);
});

// --- About ---

window.__TAURI__.event.listen("about", (event) => {
  $("about-version").textContent = `version ${event.payload}`;
  const dialog = $("about");
  // Choosing About again while it is open must not throw.
  if (!dialog.open) dialog.showModal();
});

$("about-close").addEventListener("click", () => $("about").close());

// --- Tabs ---
// Both panels start hidden: start() picks the tab once it knows what is saved, so the window
// never shows the wrong one for a moment.

const TAB_NAMES = ["credentials", "editing"];

function selectTab(name, { focus = false } = {}) {
  for (const tabName of TAB_NAMES) {
    const selected = tabName === name;
    const tab = $(`tab-${tabName}`);
    tab.setAttribute("aria-selected", String(selected));
    // Only the selected tab is a tab stop; the arrow keys move between the tabs.
    tab.tabIndex = selected ? 0 : -1;
    $(`panel-${tabName}`).hidden = !selected;
    if (selected && focus) tab.focus();
  }
}

for (const tab of document.querySelectorAll('[role="tab"]')) {
  tab.addEventListener("click", () => selectTab(tab.dataset.tab));
  tab.addEventListener("keydown", (event) => {
    const index = TAB_NAMES.indexOf(tab.dataset.tab);
    let next = null;
    if (event.key === "ArrowRight") next = (index + 1) % TAB_NAMES.length;
    else if (event.key === "ArrowLeft") next = (index - 1 + TAB_NAMES.length) % TAB_NAMES.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = TAB_NAMES.length - 1;
    if (next === null) return;
    event.preventDefault();
    selectTab(TAB_NAMES[next], { focus: true });
  });
}

$("go-credentials").addEventListener("click", () => selectTab("credentials", { focus: true }));

// What is missing before the app can work. The Credentials tab shows a mark while any is.
const setup = { token: false, tokenExpired: false, wordpress: false };

function updateCredentialsAlert() {
  $("credentials-alert").hidden = setup.token && !setup.tokenExpired && setup.wordpress;
}

// Without a token the list of posts has nothing to show; say what to do instead.
function showPostsSetup(needed) {
  $("posts-setup").hidden = !needed;
  $("posts-refresh").disabled = needed;
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
    setup.token = status.has_token;
    setup.tokenExpired = Boolean(status.has_token && status.expires_at && status.expires_at * 1000 < Date.now());
    updateCredentialsAlert();
    return status.has_token;
  } catch (message) {
    show($("token-status"), String(message), true);
    setup.token = false;
    updateCredentialsAlert();
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
    showPostsSetup(false);
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
    showPostsSetup(true);
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
    setup.wordpress = settings.has_password;
    updateCredentialsAlert();
    return settings.has_password;
  } catch (message) {
    show($("wp-status"), String(message), true);
    setup.wordpress = false;
    updateCredentialsAlert();
    return false;
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
  // Four rows are visible; the box scrolls inside, and an empty one is not shown at all.
  $("posts-scroll").hidden = posts.length === 0;
  $("posts-scroll").scrollTop = 0;
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

// --- Editor toolbar ---
// The toolbar uses the browser's own editing commands. The markup they produce is cleaned again
// by the backend before anything is sent, so the toolbar only has to be convenient.

const editor = $("editor");
const PRESSED_COMMANDS = [
  "bold",
  "italic",
  "underline",
  "strikeThrough",
  "superscript",
  "subscript",
  "insertUnorderedList",
  "insertOrderedList",
];
const BLOCKS = ["p", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre"];

function escapeHtml(text) {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

function selectionRange() {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  return editor.contains(range.commonAncestorContainer) ? range : null;
}

function closestInEditor(node, selector) {
  const element = node?.nodeType === Node.ELEMENT_NODE ? node : node?.parentElement;
  const found = element?.closest(selector);
  return found && editor.contains(found) ? found : null;
}

// Shows what applies at the cursor: the pressed buttons and the paragraph style.
function updateToolbar() {
  const range = selectionRange();
  if (!range) return;
  for (const command of PRESSED_COMMANDS) {
    const button = document.querySelector(`.toolbar [data-command="${command}"]`);
    button.setAttribute("aria-pressed", String(document.queryCommandState(command)));
  }
  for (const button of document.querySelectorAll(".toolbar [data-wrap]")) {
    const inside = closestInEditor(range.startContainer, button.dataset.wrap) !== null;
    button.setAttribute("aria-pressed", String(inside));
  }
  const block = String(document.queryCommandValue("formatBlock")).toLowerCase().replace(/[<>]/g, "");
  $("editor-block").value = BLOCKS.includes(block) ? block : "p";
}

document.addEventListener("selectionchange", updateToolbar);

// Wraps the selected text in an inline element, or takes the element off when the cursor is
// inside one. insertHTML is used so that the browser's undo still works.
function toggleWrap(tag) {
  const range = selectionRange();
  if (!range) return;
  const existing = closestInEditor(range.startContainer, tag);
  if (existing) {
    const whole = document.createRange();
    whole.selectNode(existing);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(whole);
    document.execCommand("insertHTML", false, existing.innerHTML);
  } else if (!range.collapsed) {
    document.execCommand("insertHTML", false, `<${tag}>${escapeHtml(range.toString())}</${tag}>`);
  }
}

for (const button of document.querySelectorAll(".toolbar button[data-command], .toolbar button[data-wrap]")) {
  // Keep the text selection in the editor when a toolbar button is pressed.
  button.addEventListener("mousedown", (event) => event.preventDefault());
  button.addEventListener("click", () => {
    editor.focus();
    if (button.dataset.wrap) {
      toggleWrap(button.dataset.wrap);
    } else if (["indent", "outdent"].includes(button.dataset.command)) {
      // Outside a list these would wrap the text in a styled quote that the site does not keep.
      if (closestInEditor(selectionRange()?.startContainer, "li")) {
        document.execCommand(button.dataset.command);
      }
    } else {
      document.execCommand(button.dataset.command);
    }
    updateToolbar();
  });
}

$("editor-block").addEventListener("change", () => {
  editor.focus();
  document.execCommand("formatBlock", false, `<${$("editor-block").value}>`);
  updateToolbar();
});

// --- Links and tables ---

let savedRange = null;
let editedLink = null;

function rememberSelection() {
  const range = selectionRange();
  savedRange = range ? range.cloneRange() : null;
}

function restoreSelection() {
  editor.focus();
  if (!savedRange) return;
  const selection = window.getSelection();
  selection.removeAllRanges();
  selection.addRange(savedRange);
}

// Turns what was typed into an address that the site keeps: http, https or mailto only.
function normalizeLink(raw) {
  const text = raw.trim();
  if (/^mailto:[^\s@]+@[^\s@]+$/i.test(text)) return text;
  if (/^https?:\/\/[^\s/]+\S*$/i.test(text)) return text;
  if (/^[a-z][a-z0-9+.-]*:/i.test(text)) return null;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(text)) return `mailto:${text}`;
  if (/^[^\s/]+\.[^\s/]+\S*$/.test(text)) return `https://${text}`;
  return null;
}

function openLinkDialog() {
  rememberSelection();
  editedLink = savedRange ? closestInEditor(savedRange.startContainer, "a") : null;
  $("link-url").value = editedLink?.getAttribute("href") ?? "";
  $("link-title").value = editedLink?.getAttribute("title") ?? "";
  $("link-new-tab").checked = editedLink?.getAttribute("target") === "_blank";
  $("link-text").value = "";
  $("link-text-row").hidden = Boolean(editedLink) || !savedRange?.collapsed;
  $("link-error").textContent = "";
  $("link-dialog").showModal();
  $("link-url").select();
}

function applyLink() {
  const url = normalizeLink($("link-url").value);
  if (!url) {
    $("link-error").textContent = "Enter a web address (https://...) or an e-mail address.";
    return;
  }
  const title = $("link-title").value.trim();
  const newTab = $("link-new-tab").checked;
  const finish = (anchor) => {
    if (title) anchor.setAttribute("title", title);
    else anchor.removeAttribute("title");
    if (newTab) anchor.setAttribute("target", "_blank");
    else anchor.removeAttribute("target");
  };
  const text = $("link-text").value.trim() || url;
  $("link-dialog").close();
  if (editedLink) {
    editedLink.setAttribute("href", url);
    finish(editedLink);
  } else {
    restoreSelection();
    if (selectionRange()?.collapsed) {
      document.execCommand("insertHTML", false, `<a href="${escapeHtml(url)}">${escapeHtml(text)}</a>`);
    } else {
      document.execCommand("createLink", false, url);
    }
    const selection = window.getSelection();
    for (const anchor of editor.querySelectorAll("a")) {
      if (anchor.getAttribute("href") === url && selection.containsNode(anchor, true)) finish(anchor);
    }
  }
  editedLink = null;
  updateToolbar();
}

$("tool-link").addEventListener("mousedown", (event) => event.preventDefault());
$("tool-link").addEventListener("click", openLinkDialog);
$("link-apply").addEventListener("click", applyLink);
$("link-cancel").addEventListener("click", () => $("link-dialog").close());
$("link-url").addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    applyLink();
  }
});
editor.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    openLinkDialog();
  }
});

$("tool-table").addEventListener("mousedown", (event) => event.preventDefault());
$("tool-table").addEventListener("click", () => {
  rememberSelection();
  $("table-dialog").showModal();
  $("table-rows").select();
});
$("table-cancel").addEventListener("click", () => $("table-dialog").close());
$("table-apply").addEventListener("click", () => {
  const rows = Math.min(20, Math.max(1, Number.parseInt($("table-rows").value, 10) || 1));
  const cols = Math.min(8, Math.max(1, Number.parseInt($("table-cols").value, 10) || 1));
  const header = $("table-header").checked && rows > 1;
  const cells = (tag) => `<${tag}><br></${tag}>`.repeat(cols);
  const bodyRows = rows - (header ? 1 : 0);
  const html =
    "<table>" +
    (header ? `<thead><tr>${cells("th")}</tr></thead>` : "") +
    `<tbody>${`<tr>${cells("td")}</tr>`.repeat(bodyRows)}</tbody></table><p><br></p>`;
  $("table-dialog").close();
  restoreSelection();
  document.execCommand("insertHTML", false, html);
});

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

$("publish-open").addEventListener("click", async () => {
  try {
    await invoke("open_in_browser", { url: $("publish-url").value });
  } catch (message) {
    show($("publish-message"), `Could not open the browser: ${message}`, true);
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

// --- Start ---

async function start() {
  // Both are read from the credential store, which is quick: pick the tab before any network call
  // so the window opens where the user has to be.
  await refreshTokenStatus();
  await refreshWordPressSettings();
  selectTab(setup.token && !setup.tokenExpired && setup.wordpress ? "editing" : "credentials");
  showPostsSetup(!setup.token);
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
