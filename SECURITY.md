# Security policy

## Reporting a vulnerability

Please report security issues **privately**, through GitHub's security advisories:
[Report a vulnerability](https://github.com/DokaDev/slakio/security/advisories/new) (the
"Security" tab of the repository, then "Report a vulnerability").

Do not open a public issue for a vulnerability. Include what you found, how to reproduce it,
and the version or commit you tested. You will get an answer as soon as possible, and credit in
the fix unless you prefer otherwise.

## Scope

slakio is a chat client: it will hold workspace session tokens and cookies, draw text that
other people wrote, and download files they shared. Reports about any of these are especially
welcome:

- tokens, cookies or message content leaking to disk, logs, the screen or another process;
- terminal escape sequences in remote content (messages, names, file names) that reach the
  terminal instead of being shown as text;
- downloads written outside the folder the user chose, or opened without being asked.

## Supported versions

slakio is pre-alpha and has no release yet. Once it has, only the latest release is
supported until 1.0, and fixes go into the next release.
