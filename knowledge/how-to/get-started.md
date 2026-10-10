---
title: Set up Instapress from scratch
status: active
---

# Set up Instapress from scratch

A step-by-step guide to everything the app needs before it can turn an Instagram post into a WordPress draft. Do it once. Each part says how to check that it worked.

You need:
- an Instagram account that is a **professional account** (Creator or Business) and that you own;
- a WordPress site you administer, served over **HTTPS**;
- about 30 minutes.

The two services are set up separately and the app joins them: [part 1](#1-the-instagram-access-token) gives you a token, [part 2](#2-wordpress-application-password) a password, [parts 3 and 4](#3-let-wordpress-receive-the-login-htaccess) fix two things a typical WordPress site gets wrong, and [part 6](#6-first-run) puts it all in the app.

Never paste the token or the password anywhere but the app (and the Meta and WordPress screens that produce them): not in chat, an issue, a commit or a screenshot.

---

## 1. The Instagram access token

The app reads **your own** posts through the Instagram API with Instagram Login. It works in Meta's *development mode*: no app review is needed as long as your account is a tester of your own Meta app. It only asks for the `instagram_business_basic` permission (read the profile and the media).

Meta changes the names and positions of its screens often. The steps below name the things to look for.

### 1.1 Turn the account into a professional account
In the Instagram app: **Settings and privacy → Account type and tools → Switch to professional account**, and choose Creator or Business. A personal account cannot use this API.

### 1.2 Create a Meta developer app
1. Go to [developers.facebook.com](https://developers.facebook.com/) and log in. If it is your first time, register as a developer (it is free).
2. **My Apps → Create app**. Give it any name (for example "Instapress"), and a contact email.
3. When it asks for the use case, choose the one that is about managing messaging and content on **Instagram** (or *Other* if you are given the older flow, then app type **Business**).
4. In the app dashboard open **Add product / Use cases** and add **Instagram**, then open **API setup with Instagram login**.

You do not need to publish the app or send anything to review. Leave it in **development mode**.

### 1.3 Make your account a tester
1. In the Meta app go to **App roles → Roles → Instagram testers** (the exact path may read "Roles" and then "Add People").
2. Add your Instagram account by its user name and send the invitation.
3. Accept it from the Instagram side: on the web go to `instagram.com`, then **Settings → Apps and websites → Tester invites** (on the phone, **Settings and privacy → Website permissions → Apps and websites**). Accept the invitation of your app.

The tester invitation is the step people miss: without it, generating the token fails with a message about missing access.

### 1.4 Generate the token
1. In **API setup with Instagram login**, under *Generate access tokens*, click **Add account** and log in with the Instagram account that accepted the invitation (do this in a browser where that Instagram session is open).
2. Accept the permission it asks for (read your profile and media).
3. Click **Generate token** next to the account and copy the token. It is a long string and it is shown only now.

This token is *long-lived*: it lasts about 60 days. The app renews it by itself every time it starts, as long as you open it at least once before it expires.

**Check:** later, in the app, **Credenciales → Instagram access → Save**, then the **Edición** tab must list your recent posts.

---

## 2. WordPress Application Password

The app never uses your normal WordPress password. It uses an *Application Password*, which you can revoke at any time without changing your login.

1. Log in to `https://your-site.com/wp-admin`.
2. **Users → Profile** (your own user).
3. Scroll to **Application Passwords**. Type a name ("Instapress") and click **Add New Application Password**.
4. Copy the password it shows (groups of letters separated by spaces; the spaces do not matter). It is shown only once.

Things to know:
- The user must be able to create posts: role **Author**, **Editor** or **Administrator**.
- The **user name** the app needs is the one you log in with, shown in **Users → Profile** as *Username*. It is **not** your e-mail address and not your display name. A wrong user name gives a 401 error that looks like a wrong password.
- If the *Application Passwords* section is missing, the site is not on HTTPS, or a plugin has turned the feature off (some security plugins do; look for an option such as "disable application passwords" and switch it off for this user). Sites with a login-protection plugin or single sign-on (for example Jetpack's "Log in with WordPress.com", Wordfence, iThemes Security) may also block the REST API: allow REST API requests with Basic authentication for your user.

---

## 3. Let WordPress receive the login (`.htaccess`)

Many hosts running Apache do not pass the `Authorization` header to WordPress. Then every request fails with `rest_cannot_create` (or `401`) even with the right user and password.

Skip this part if the check at the end already works.

1. Open the `.htaccess` file in the site's root folder (the one that contains `wp-config.php`). Use the host's file manager or FTP. **Make a copy first.**
2. Add these lines at the **top** of the file, above `# BEGIN WordPress`:

```apache
# Pass the Authorization header to PHP so the REST API can read the Application Password
<IfModule mod_rewrite.c>
RewriteEngine On
RewriteCond %{HTTP:Authorization} ^(.*)
RewriteRule ^(.*) - [E=HTTP_AUTHORIZATION:%1]
</IfModule>
```

3. Save. Do not edit the block between `# BEGIN WordPress` and `# END WordPress`: WordPress rewrites it.

If the host runs Nginx or LiteSpeed with its own panel, the file does not apply: ask the host to pass the `Authorization` header to PHP (in Nginx it is `fastcgi_param HTTP_AUTHORIZATION $http_authorization;`).

**Check:** in the app, **Credenciales → WordPress access** save the three values, then in the **Edición** tab create a draft of a post. If it says `rest_cannot_create` or "Sorry, you are not allowed to create posts", the header is not arriving: re-check this part and the user's role.

---

## 4. Let the SEO fields reach Yoast

Optional, but without it the **slug is fine and the Yoast fields are ignored**. If you use the Yoast SEO plugin and want the app to fill the *Meta description* and the *Focus keyphrase*, WordPress must be told to accept them over the REST API. It ignores them otherwise, without an error.

1. Install a code-snippets plugin: **WPCode** (Plugins → Add New → search "WPCode") or **Code Snippets**. You can also put the code in the `functions.php` of a child theme.
2. In WPCode: **Code Snippets → Add Snippet → Add Your Custom Code (New Snippet)**, choose **PHP Snippet**.
3. The editor already shows `<?php` on the first line. **Do not delete it and do not paste a second one**: write the code *under* it.

```php
add_action( 'init', function () {
	$keys = array( '_yoast_wpseo_metadesc', '_yoast_wpseo_focuskw' );
	foreach ( $keys as $key ) {
		register_post_meta( 'post', $key, array(
			'show_in_rest'  => true,
			'single'        => true,
			'type'          => 'string',
			'auth_callback' => function () {
				return current_user_can( 'edit_posts' );
			},
		) );
	}
} );
```

4. Set **Insert Method: Auto Insert**, **Location: Run Everywhere**, switch the snippet **Active** and **Save**.

**Check:** create a draft from the app. If a warning appears saying the meta description or the focus keyphrase "was not saved", the snippet is not active. Then open the draft in WordPress and **save it once**: Yoast fills its panel and its score from its own index, which is updated at that save. See [`../operational/yoast-meta-description.md`](../operational/yoast-meta-description.md) for the reasons.

Without Yoast the two fields have no effect; you can leave them empty.

---

## 5. Emoji and the database

The app removes emoji from the title and the SEO fields, and writes them as safe entities in the body. This is on purpose: a WordPress database on the old 3-byte `utf8` character set refuses emoji with the error `db_insert_error`. You do not have to change anything. If you want to keep emoji everywhere, ask the host to convert the database to `utf8mb4`.

---

## 6. First run

1. Download the latest version from the [Releases page](https://github.com/enoliglesias/instapress/releases) and open **Instapress**:
   - **Windows**: run the `.exe` installer. Windows SmartScreen may say "Windows protected your PC": choose **More info**, then **Run anyway**. The app is not code-signed.
   - **macOS** (Apple Silicon): open the `.dmg` and drag Instapress to Applications. The app is not notarized, so the first time right click it and choose **Open**, or run `xattr -dr com.apple.quarantine /Applications/Instapress.app` in the Terminal.
2. Open the **Credenciales** tab.
   - **Instagram access**: paste the token and click **Save**. The line above the box says when it expires.
   - **WordPress access**: the site address (`https://your-site.com`, the folder if WordPress is in one), the user name and the Application Password, then **Save**.
3. Open **Edición**. Your recent posts load. Pick one: its text appears in the editor with a suggested title, slug and meta description.
4. Edit what you want, tick the categories, choose the picture and the video option, then **Create draft in WordPress**.
5. The app shows an address. Open it, review the draft in WordPress and publish it from there. The app never publishes.

Your token and password are stored in the operating system's credential store (Windows Credential Manager, macOS Keychain, Secret Service), never in a file.

---

## When something fails

| What you see | Cause | What to do |
|---|---|---|
| Instagram "missing access" or the login asks for permission and loops | The account has not accepted the tester invitation, or is not professional | Part 1.1 and 1.3 |
| The Edición tab says there is no token, or "Could not renew the token" | The token expired (it lasts 60 days) | Generate a new one (1.4), paste it again |
| `rest_cannot_create`, "not allowed to create posts" | The `Authorization` header does not arrive, the user cannot write posts, or a security plugin blocks it | Parts 2 and 3 |
| `incorrect_password`, `invalid_username`, 401 | Wrong user name (an e-mail or display name), or the password was not copied whole | Part 2: use the *Username*, create a new Application Password |
| "The site redirected" | The address saved is not the final one (`http` instead of `https`, or without `www`) | Enter the address as it shows in the browser |
| `db_insert_error` | Four-byte characters on a 3-byte database | The app already avoids them; update the app if you see it |
| Draft created but a warning about the meta description or the focus keyphrase | The snippet of part 4 is missing or inactive | Part 4 |
| Video does not play on the site | The Instagram player may open Instagram on click; it cannot hide Instagram's header or comments | Expected, nothing to change |
| The pictures of the list are missing | Instagram picture links are temporary | Press **Refresh** |
