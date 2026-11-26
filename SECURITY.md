# Security

prisme's threat model.

Last updated: 2026-11-01.

Documents and diagrams are untrusted input: a downloaded README must not be able to run code or reach the shell. The shell itself is full access to the machine by design; it is only ever driven by you.

## Untrusted documents

- Raw HTML in Markdown is off by default, with a setting to allow it (the setting is not built yet; Phase 1 always drops raw HTML).
- All HTML is sanitized in Rust (`ammonia`) before it reaches the webview; Mermaid's SVG is sanitized again in the frontend (DOMPurify), and Mermaid runs in strict mode in a separate hidden webview.
- A strict Content Security Policy: only prisme's own bundled scripts run; nothing remote. Images may come from the app, `data:`, the asset protocol and http(s) (remote ones only after you allow them, below).
- File access is limited to the open workspace: every path the frontend asks for is resolved in Rust and refused if it leaves the workspace, symlinks included; the asset protocol (local images) is scoped to the workspace folder, and image paths outside it are blocked.
- Links: `#heading` scrolls, relative links open workspace files in prisme, `http(s):` and `mailto:` open in your browser (checked again in Rust), and every other scheme (`javascript:`, `file:`, custom) is never followed.

(F3)

Saving (Phase 3) goes through the same workspace check: the editor can only write a file inside the open workspace, and only the file it opened. A save is refused when the file changed on disk since the editor loaded it, until you choose "Keep mine"; it is written beside the original and renamed over it, so a crash never leaves half a file. The diff window only reads.

Exports (diagrams as images) are the one way prisme writes outside the workspace, and only to a path you pick in the system's save dialog, which Rust opens itself: the page hands over the bytes, never a path. Diagram exports are images; workspace images are read by Rust, images only, from inside the workspace. Math is typeset by KaTeX with `trust` off, so TeX cannot make links or load images.

## Remote images

Loading a remote image tells its server you opened the document (tracking pixels). prisme asks per document; the prompt has "don't ask again", which remembers the answer for good and can be changed in settings (for now the "Remote images" control in the sidebar). Until allowed, a remote image has no `src` at all, so nothing is requested. (F4)

## Documents and the shell

Document content can never start a terminal session or type into one. The "send to terminal" button on code blocks pastes only when you click it, and never presses Enter: you run the command yourself. (F5, D3)

As built: the button is added by prisme after sanitizing, so a document cannot make one or click one. It pastes only into a session you already opened and never starts one. Multi-line text is pasted only as a bracketed paste, which the shell keeps as one edit instead of running line by line; without bracketed paste nothing is pasted. A session takes input only from the window that opened it. Links in terminal output open only workspace files (checked in Rust) or http(s) URLs, and only on Ctrl+click.

## LaTeX compiling

Opening a `.tex` file runs the TeX installed on your system on it, else the Tectonic that ships with prisme (C4): a `.tex` tab is always its compiled PDF. A tab opens because you opened it, followed a link to it, Ctrl+clicked its path in a terminal, or had Follow changes on while it was written; a document cannot open one any other way. TeX runs with shell escape off (`-no-shell-escape`; Tectonic has it off by default), so a document cannot start programs, and only into its output folder, which is in prisme's cache, never the workspace: TeX Live and MiKTeX through `openout_any=p`; Tectonic, which has no such setting and writes a `\openout` file under any name it is given (`../`, absolute paths), only as the bundled one, built with a patch that skips a name outside the output folder (`scripts/tectonic-confine-outputs.patch`). A Tectonic of the system is therefore used only by a build that lacks the bundled one, and is not confined. `latexmk` runs with `-norc`: a `latexmkrc` is Perl, and one placed beside a document would otherwise run as code. The source is copied into the cache and compiled there, so no rc file or `.aux` of the document's folder is read as the compile's own. TeX can still read any file you can (its default `openin_any`), so a hostile `.tex` could `\input` a private file and set it into the PDF; that PDF only goes to prisme's view. The bundled Tectonic is built from Tectonic's release tag with that patch by prisme's `tectonic` workflow, which checks that a document cannot write outside its output folder, and published as a pre-release of prisme's repository; a build fetches it checked against a pinned SHA-256. It starts from the packages prisme ships, which were downloaded from Tectonic's bundle server when prisme was built (in the release job itself, not checked against a pinned digest: they are TeX macros and fonts, read by a TeX with shell escape off, and pinning them is left for later); a package beyond those, and a weekly check for a newer bundle, are Tectonic's own requests to that server. What it downloads stays in prisme's cache. A compile is stopped after 120 s, or 600 s while the bundled Tectonic has no format file yet (its first compile when no starting cache shipped, which downloads its packages and builds the format).

## Git

The Git panel runs the system's `git` only in the workspace folder and only when you open the panel or press a button; nothing a document contains reaches its arguments (the paths it compares come from `git status`, and one that leaves the folder is refused). Reading the status never runs the repository's `core.fsmonitor` command (forced off, since `git status` runs on its own when the panel is open). Commit and sync run the repository's hooks as `git` would in a terminal; hooks live in `.git/hooks`, which a clone never brings, so they are only what you or your tools put there. git never prompts: passwords and passphrases are left to the terminal and to your credential helper. Pushing sends your commits to the remote you configured, and only when you press Sync.

## Updates

prisme's only network request of its own is the update check (Phase 5, M3): at most once a day, and only in release builds that name an update server, it fetches one JSON file over HTTPS; the request carries no data about you or your documents. A setting turns it off. Every update is signed: prisme has the release key's public half built in and refuses a package whose signature does not match, so a compromised server or download cannot install anything. The server is the GitHub release (`latest.json` beside the packages). The AppImage replaces only itself, as you; on Windows the signed installer runs as it did when you installed; on macOS the app bundle is replaced. prisme never asks for a root password: for a `.deb` it only opens the new package's download link, and installing it is up to you. The private signing key lives only in the repository's secrets and with whoever releases prisme.
