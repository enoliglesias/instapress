---
title: Sending the Yoast SEO fields
status: active
---

# Sending the Yoast SEO fields

The app sends two Yoast SEO fields in the `meta` of the post:
- the **meta description**, under the key `_yoast_wpseo_metadesc`;
- the **focus keyphrase** (the target phrase of the post), under the key `_yoast_wpseo_focuskw`.

WordPress only accepts a meta key over the REST API when the site has registered it with `show_in_rest`, and Yoast does not do that. Without the registration WordPress **ignores the field without any error**: the draft is created and the field is simply missing.

## How the app notices
The answer to the request lists the meta of the post that is registered for the REST API. For each field that was sent, the app checks that the answer holds the same text. `CreatedPost::meta_description_saved` and `CreatedPost::focus_keyphrase_saved` are `None` when the field was not sent, `true` when it was kept and `false` when it was not. If any is `false` the window shows a warning under the result that names the fields. Each field is judged on its own, because a site may register one and not the other.

## One-time setup on the site
Add this code once to the site, in a code snippets plugin (for example WPCode or Code Snippets) or in the `functions.php` of a child theme. In WPCode choose a PHP snippet and write the code under the `<?php` line that the editor already shows:

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

`auth_callback` is needed because the keys start with an underscore, which WordPress treats as protected meta. The user whose Application Password the app uses must be able to edit posts.

## Checking it
`OPTIONS /wp-json/wp/v2/posts` lists `meta` in its schema; both keys must appear there. Or create a draft from the app: when the warning does not show, the fields were kept and appear in Yoast's panel when the draft is opened in WordPress.

## Notes
- Yoast builds what it shows from its own index. A value written over the REST API reaches the index when the post is saved again, so **open the draft in WordPress and save or publish it**, then check Yoast's panel and the page's `<meta name="description">`. Yoast also computes its analysis score in the editor, so it appears after that save.
- Both fields are stored as they are, so the app removes emoji (four-byte characters) from them, as a database on the 3-byte `utf8` charset would refuse them. An HTML entity cannot replace them here because the text is escaped when the page is built.
