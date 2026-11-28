# Windows Alpha conversion feedback

An Alpha build offers feedback in **変換の報告** when built with
`SWEET_IME_ALPHA_FEEDBACK_URL`. The user enters an Alpha invitation and
explicitly consents before any conversions are collected.

The client batches up to 32 records per active Windows application, at
roughly one-minute intervals. A record contains the kana reading and
confirmed text, with the first candidate when the user selected a
different candidate in the same composition. The batch includes the Alpha
version. It contains no surrounding text, raw keystrokes, app names,
installation IDs, or device identifiers. Fields with missing, unknown, or
sensitive scopes are excluded. Records containing Latin letters, digits,
or control characters are discarded. Japanese text can still contain
personal information.

Unsent data is held in memory and can be lost when the app exits or an
upload fails. The client checks that consent is unchanged before each
upload. Revoking consent stops future collection and uploads. Requests
already in flight and data already received cannot be recalled.

## Deploy

Create a dedicated private R2 bucket, set its expiration, and deploy the
Worker from this directory. Keep public bucket access and request logging
disabled. Use a random 32–128 character invitation using letters, digits,
underscores, and hyphens, shared only with Alpha testers.

```sh
npx wrangler r2 bucket create sweet-ime-alpha-feedback
npx wrangler r2 bucket lifecycle add sweet-ime-alpha-feedback expire-feedback v1/ --expire-days 30
npx wrangler deploy
npx wrangler secret put ALPHA_INVITE
```

Set the GitHub repository variable `SWEET_IME_ALPHA_FEEDBACK_URL` to the
Worker's HTTPS URL plus `/v1/batches` before building an Alpha. For a local
build, set the same environment variable before building both
`sweet-settings` and `sweet-tsf`. The invite stays out of the build.
Without an endpoint, the Alpha option is hidden.

The Worker accepts authenticated POST requests and provides no read API.
It stores validated JSON under `v1/` in R2. View or download the batches
through authenticated Cloudflare account access. The R2 configuration and
retention rule must be active before distributing an enabled build.
