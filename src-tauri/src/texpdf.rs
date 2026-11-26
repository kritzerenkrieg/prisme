//! `.tex` files compiled to PDF (C4) by the TeX the system has: `latexmk`,
//! else Tectonic, else `pdflatex` (or `xelatex` / `lualatex` when the file
//! asks for it). Without any of those it uses the Tectonic that ships
//! beside prisme (`prisme-tectonic`). Its cache, in prisme's, starts as a
//! copy of the one that ships with prisme (the common packages and its
//! format file, so most documents compile offline from the start); a
//! package beyond those is downloaded when first needed. The source (the file, or the
//! editor's unsaved text) is written into a folder of prisme's cache, one per
//! file, and compiled there; the file's own folder is on TeX's search path,
//! so `\input`, `\include`, pictures and bibliographies beside it are found.
//! Nothing is written into the workspace. Shell escape is off and
//! `latexmk` reads no rc file (a `latexmkrc` is Perl, so a document could run
//! code through one).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A compile running longer than this is stopped.
const TIMEOUT: Duration = Duration::from_secs(120);
/// The bundled Tectonic's first compile downloads its packages and builds
/// its format file: it gets longer, until that format exists.
const FIRST_TIMEOUT: Duration = Duration::from_secs(600);
/// The name the bundled Tectonic is installed under, beside prisme's own
/// program (not `tectonic`, so it never clashes with a system package).
const BUNDLED: &str = "prisme-tectonic";
/// At most this many lines of the log come back with an error.
const ERROR_LINES: usize = 24;
/// Of what TeX prints, only this much of the end is kept (for an error with no log).
const OUTPUT_TAIL: usize = 64 * 1024;

/// One compile at a time per file: TeX's auxiliary files are shared. And
/// one first compile of the bundled Tectonic at a time: the files it
/// downloads are the same for every document, so a second one waits for
/// them instead of fetching them again beside it.
#[derive(Default)]
pub struct Compiler {
    files: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    first: Mutex<()>,
}

impl Compiler {
    fn lock(&self, dir: &Path) -> Arc<Mutex<()>> {
        self.files.lock().unwrap().entry(dir.to_path_buf()).or_default().clone()
    }
}

/// Whether the bundled Tectonic has built its format file in `cache`: until
/// then, a compile downloads packages first.
fn has_format(cache: &Path) -> bool {
    std::fs::read_dir(cache.join("formats")).is_ok_and(|mut d| d.next().is_some())
}

/// Whether the system has a TeX that compiles `program`'s documents.
fn system_tex(program: Program) -> bool {
    find("latexmk").is_some() || find("tectonic").is_some() || find(program.binary()).is_some()
}

/// Whether a compile now would be the bundled Tectonic's first with nothing
/// to start from: no TeX of the system, no format yet in `tectonic_cache`,
/// and no shipped cache (`seed`) to copy one from.
pub fn first_run(tectonic_cache: &Path, seed: Option<&Path>) -> bool {
    !system_tex(Program::Pdf) && bundled().is_some() && !has_format(tectonic_cache) && !seed.is_some_and(has_format)
}

/// Copy what `from` holds into `to`, keeping the files `to` already has.
fn copy_missing(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            // Through a temporary name, so a copy cut short never passes for a whole file.
            let part = to.join(format!(".{}.part", entry.file_name().to_string_lossy()));
            std::fs::copy(entry.path(), &part)?;
            std::fs::rename(&part, &target)?;
        }
    }
    Ok(())
}

/// The TeX engine a document asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Pdf,
    Xe,
    Lua,
}

impl Program {
    fn binary(self) -> &'static str {
        match self {
            Program::Pdf => "pdflatex",
            Program::Xe => "xelatex",
            Program::Lua => "lualatex",
        }
    }
}

/// `% !TEX program = xelatex` (or `TS-program`) near the top, as TeXShop and
/// VS Code read it; else `fontspec` or `unicode-math`, which need XeLaTeX or
/// LuaLaTeX; else pdfLaTeX.
pub fn program(text: &str) -> Program {
    for line in text.lines().take(30) {
        let l = line.trim().to_ascii_lowercase();
        let Some(rest) = l.strip_prefix('%') else { continue };
        let rest = rest.trim_start();
        if let Some(rest) = rest.strip_prefix("!tex") {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix("ts-program").or_else(|| rest.strip_prefix("program"));
            if let Some(value) = rest.and_then(|r| r.trim_start().strip_prefix('=')) {
                let v = value.trim();
                if v.starts_with("xe") {
                    return Program::Xe;
                }
                if v.starts_with("lua") {
                    return Program::Lua;
                }
                if v.starts_with("pdf") {
                    return Program::Pdf;
                }
            }
        }
    }
    let wants_unicode = text.lines().any(|l| {
        let code = l.split('%').next().unwrap_or("");
        code.contains("\\usepackage") && (code.contains("fontspec") || code.contains("unicode-math"))
    });
    if wants_unicode { Program::Xe } else { Program::Pdf }
}

/// A program on the PATH, or in the places TeX installs itself that a
/// desktop session's PATH may lack (macOS's `/Library/TeX/texbin`;
/// `~/.local/bin` and `~/bin`, where TinyTeX links its programs).
fn find(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(["/Library/TeX/texbin", "/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"].map(PathBuf::from));
    if let Some(home) = std::env::var_os("HOME") {
        for d in [".local/bin", "bin", ".cargo/bin"] {
            dirs.push(Path::new(&home).join(d));
        }
    }
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

/// The Tectonic that ships beside prisme's program, if it is there.
fn bundled() -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{BUNDLED}.exe") } else { BUNDLED.to_string() };
    let path = std::env::current_exe().ok()?.parent()?.join(exe);
    path.is_file().then_some(path)
}

/// Tectonic's arguments: output folder, logs kept, the source's folder searched.
fn tectonic_command(exe: &Path, out: &Path, src_dir: &Path) -> Command {
    let mut c = program_command(exe);
    c.args(["--chatter", "minimal", "--keep-logs", "--outdir"]).arg(out).arg("-Z").arg(format!("search-path={}", src_dir.display()));
    c
}

/// `dir` ahead of what `var` already holds; the empty entry at the end keeps TeX's defaults.
fn search_path(var: &str, dir: &Path) -> OsString {
    let mut parts = vec![dir.to_path_buf()];
    if let Some(old) = std::env::var_os(var) {
        parts.extend(std::env::split_paths(&old).filter(|p| !p.as_os_str().is_empty()));
    }
    parts.push(PathBuf::new());
    std::env::join_paths(parts).unwrap_or_else(|_| dir.as_os_str().to_owned())
}

/// A command for the program at `exe`, with its folder first on the PATH:
/// found outside the PATH (TinyTeX's `~/.local/bin`, say), `latexmk` must
/// still find `pdflatex` and the rest beside it by name.
fn program_command(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    if let Some(dir) = exe.parent() {
        let mut parts = vec![dir.to_path_buf()];
        if let Some(old) = std::env::var_os("PATH") {
            parts.extend(std::env::split_paths(&old));
        }
        if let Ok(path) = std::env::join_paths(parts) {
            c.env("PATH", path);
        }
    }
    c
}

/// The folders under the source's that `\include{dir/file}` writes an
/// `.aux` into: TeX does not make them in its output folder itself, and
/// stops when one is missing. Only plain relative paths inside it.
fn include_dirs(text: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for line in text.lines() {
        let code = line.split('%').next().unwrap_or("");
        let mut rest = code;
        while let Some(i) = rest.find("\\include{") {
            rest = &rest[i + "\\include{".len()..];
            let Some(end) = rest.find('}') else { break };
            let parent = Path::new(rest[..end].trim()).parent().map(Path::to_path_buf).unwrap_or_default();
            let plain = parent.components().all(|c| matches!(c, std::path::Component::Normal(_)));
            if plain && !parent.as_os_str().is_empty() && !dirs.contains(&parent) {
                dirs.push(parent);
            }
            rest = &rest[end..];
        }
    }
    dirs
}

/// Stop the compile and everything it started (`latexmk` runs the engine,
/// which would go on writing into the output folder).
fn kill_all(child: &mut std::process::Child) {
    #[cfg(unix)]
    // SAFETY: the child leads its own process group (see `compile`); this
    // signals that group only.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).creation_flags(CREATE_NO_WINDOW).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The command that compiles `file` (in `out`, its own folder) for a source
/// in `src_dir`, and how long it may run. `tectonic_cache` is where the
/// bundled Tectonic keeps its packages.
fn command(file: &Path, out: &Path, src_dir: &Path, program: Program, tectonic_cache: &Path) -> Result<(Command, Duration), String> {
    let mut timeout = TIMEOUT;
    let interaction = ["-interaction=nonstopmode", "-halt-on-error", "-file-line-error"];
    let mut cmd = if let Some(latexmk) = find("latexmk") {
        let mut c = program_command(&latexmk);
        c.arg("-norc")
            .arg(match program {
                Program::Pdf => "-pdf",
                Program::Xe => "-pdfxe",
                Program::Lua => "-pdflua",
            })
            .arg("-no-shell-escape")
            .arg("-silent")
            .args(interaction)
            .arg(format!("-outdir={}", out.display()));
        c
    } else if let Some(tectonic) = find("tectonic") {
        tectonic_command(&tectonic, out, src_dir)
    } else if let Some(tex) = find(program.binary()) {
        let mut c = program_command(&tex);
        c.arg("-no-shell-escape").args(interaction).arg(format!("-output-directory={}", out.display()));
        c
    } else if let Some(tectonic) = bundled() {
        // Its format file is built once, after the first packages arrive.
        if !has_format(tectonic_cache) {
            timeout = FIRST_TIMEOUT;
        }
        let mut c = tectonic_command(&tectonic, out, src_dir);
        c.env("TECTONIC_CACHE_DIR", tectonic_cache);
        c
    } else {
        return Err(format!(
            "No TeX found to compile with: prisme uses latexmk, Tectonic or {}, else the Tectonic it ships with, which is missing from this build. Install TeX Live (or MiKTeX, MacTeX), or Tectonic.",
            program.binary()
        ));
    };
    cmd.arg(file)
        .current_dir(out)
        .env("TEXINPUTS", search_path("TEXINPUTS", src_dir))
        .env("BIBINPUTS", search_path("BIBINPUTS", src_dir))
        .env("BSTINPUTS", search_path("BSTINPUTS", src_dir))
        // TeX writes only into its output folder.
        .env("openout_any", "p")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok((cmd, timeout))
}

/// Compile `text` (the source of `src`, a `.tex` file) in `out`; the PDF's bytes,
/// or the errors from TeX's log. `tectonic_cache` is where the bundled
/// Tectonic keeps its packages, and `seed` the cache that ships with prisme,
/// copied into it before its first compile.
pub fn compile(compiler: &Compiler, src: &Path, text: &str, out: &Path, tectonic_cache: &Path, seed: Option<&Path>) -> Result<Vec<u8>, String> {
    let lock = compiler.lock(out);
    let _one = lock.lock().unwrap();
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    let file = out.join(format!("{stem}.tex"));
    std::fs::write(&file, text).map_err(|e| e.to_string())?;
    let pdf = out.join(format!("{stem}.pdf"));
    let log = out.join(format!("{stem}.log"));
    // A PDF left by an earlier compile must not pass for this one's.
    let _ = std::fs::remove_file(&pdf);
    for dir in include_dirs(text) {
        let _ = std::fs::create_dir_all(out.join(dir));
    }

    let src_dir = src.parent().unwrap_or(Path::new("."));
    if let Some(seed) = seed
        && !has_format(tectonic_cache)
        && has_format(seed)
        && !system_tex(program(text))
        && bundled().is_some()
    {
        let _first = compiler.first.lock().unwrap();
        if !has_format(tectonic_cache) {
            copy_missing(seed, tectonic_cache).map_err(|e| format!("Could not set up the bundled TeX in {}: {e}", tectonic_cache.display()))?;
        }
    }
    let (mut cmd, mut timeout) = command(&file, out, src_dir, program(text), tectonic_cache)?;
    let _first = (timeout == FIRST_TIMEOUT).then(|| compiler.first.lock().unwrap());
    // Another first compile may have built the format while this one waited.
    if _first.is_some() && has_format(tectonic_cache) {
        (cmd, timeout) = command(&file, out, src_dir, program(text), tectonic_cache)?;
    }
    let (status, output) = run(cmd, timeout)?;
    if status.success()
        && let Ok(bytes) = std::fs::read(&pdf)
    {
        return Ok(bytes);
    }
    let log = std::fs::read(&log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let errors = errors(&log);
    Err(if !errors.is_empty() {
        errors
    } else {
        let tail: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = tail[tail.len().saturating_sub(ERROR_LINES)..].join("\n");
        if tail.is_empty() { format!("TeX stopped ({status}) without a PDF.") } else { tail }
    })
}

/// Run `cmd` (its output piped) to the end, or stop it and everything it
/// started after `timeout`; its exit status and the end of what it printed.
fn run(mut cmd: Command, timeout: Duration) -> Result<(std::process::ExitStatus, String), String> {
    // Its own process group, so a timeout stops the engine `latexmk` started too.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Could not start TeX: {e}"))?;
    // Read the pipes on threads, so a chatty TeX never blocks on a full
    // pipe; only the end of what it prints is kept.
    let pipe = |r: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            if let Some(mut r) = r {
                let mut chunk = [0u8; 8192];
                while let Ok(n) = r.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    kept.extend_from_slice(&chunk[..n]);
                    if kept.len() > 2 * OUTPUT_TAIL {
                        kept.drain(..kept.len() - OUTPUT_TAIL);
                    }
                }
            }
            let start = kept.len().saturating_sub(OUTPUT_TAIL);
            String::from_utf8_lossy(&kept[start..]).into_owned()
        })
    };
    let stdout = pipe(child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
    let stderr = pipe(child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                kill_all(&mut child);
                return Err(format!("TeX ran for more than {} s and was stopped.", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let output = format!("{}{}", stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    Ok((status, output))
}

/// The error lines of a TeX log (`! …` or, with `-file-line-error`,
/// `file:line: …`), each with the lines after it that show where.
pub fn errors(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() && out.len() < ERROR_LINES {
        let l = lines[i];
        let file_line = l.split(':').nth(1).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) && l.matches(':').count() >= 2;
        if l.starts_with('!') || (file_line && !l.starts_with(' ')) {
            // The message and TeX's context lines, up to the blank line after them.
            let end = (i + 1..lines.len().min(i + 6)).find(|&j| lines[j].trim().is_empty()).unwrap_or(lines.len().min(i + 6));
            out.extend(&lines[i..end]);
            out.push("");
            i = end;
        } else {
            i += 1;
        }
    }
    out.join("\n").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_from_magic_comment_or_packages() {
        assert_eq!(program("\\documentclass{article}\n\\begin{document}x\\end{document}"), Program::Pdf);
        assert_eq!(program("% !TEX program = xelatex\n\\documentclass{article}"), Program::Xe);
        assert_eq!(program("%!TeX TS-program = LuaLaTeX\n"), Program::Lua);
        assert_eq!(program("% !TEX program = pdflatex\n\\usepackage{fontspec}"), Program::Pdf);
        assert_eq!(program("\\usepackage{fontspec}\n"), Program::Xe);
        assert_eq!(program("% \\usepackage{fontspec}\n"), Program::Pdf);
    }

    #[test]
    fn folders_for_included_files() {
        let text = "\\include{intro}\n\\include{chapters/one} \\include{chapters/two}\n% \\include{old/x}\n\\include{../up/x}\\include{/abs/x}\\include{a/b/c}";
        assert_eq!(include_dirs(text), [PathBuf::from("chapters"), PathBuf::from("a/b")]);
    }

    /// A compile that runs too long is stopped with everything it started:
    /// here a shell that starts a child and waits for it, as `latexmk` waits
    /// for its engine.
    #[cfg(unix)]
    #[test]
    fn timeout_stops_what_the_compile_started() {
        let dir = std::env::temp_dir().join(format!("prisme-tex-timeout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("child.pid");
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 60 & echo $! > \"$1\"; wait").arg("sh").arg(&pid_file).stdout(Stdio::piped()).stderr(Stdio::piped());
        let started = Instant::now();
        let err = run(cmd, Duration::from_millis(500)).unwrap_err();
        assert!(err.contains("was stopped"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        // Gone (not even a zombie for long: init reaps it) within a few seconds.
        // SAFETY: signal 0 only checks whether the process exists.
        let alive = || unsafe { libc::kill(pid, 0) } == 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!alive(), "the child {pid} still runs");
    }

    /// Runs only where TeX is installed.
    #[test]
    fn compiles_with_a_nested_include() {
        if find("latexmk").is_none() && find("tectonic").is_none() && find("pdflatex").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("prisme-tex-include-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/chapters")).unwrap();
        std::fs::write(dir.join("src/chapters/one.tex"), "Chapter one.").unwrap();
        let text = "\\documentclass{article}\n\\begin{document}\n\\include{chapters/one}\n\\end{document}\n";
        let src = dir.join("src/main.tex");
        std::fs::write(&src, text).unwrap();
        let pdf = compile(&Compiler::default(), &src, text, &dir.join("out"), &dir.join("tectonic"), None);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(pdf.unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn errors_from_a_log() {
        let log = "This is pdfTeX\n(./a.tex\n./a.tex:4: Undefined control sequence.\nl.4 \\foo\n         bar\n\nHere is how much\n! Emergency stop.\n<*> a.tex\n\nNo pages of output.\n";
        let e = errors(log);
        assert!(e.starts_with("./a.tex:4: Undefined control sequence.\nl.4 \\foo"), "{e}");
        assert!(e.contains("! Emergency stop."), "{e}");
        assert!(!e.contains("No pages"), "{e}");
        assert_eq!(errors("all good\n"), "");
    }

    /// A real compile, when the machine has TeX: a file beside the source is
    /// found, nothing is written beside it, and an error comes back from the log.
    #[test]
    fn compiles_with_the_system_tex() {
        if find("latexmk").is_none() && find("tectonic").is_none() && find("pdflatex").is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("prisme-tex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (docs, out) = (base.join("docs"), base.join("out"));
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("part.tex"), "Included text.\n").unwrap();
        let src = docs.join("main.tex");
        let text = "\\documentclass{article}\n\\begin{document}\nHello $x^2$.\n\\input{part}\n\\end{document}\n";
        std::fs::write(&src, text).unwrap();
        let compiler = Compiler::default();

        let pdf = compile(&compiler, &src, text, &out, &base.join("tectonic"), None).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let beside: Vec<_> = std::fs::read_dir(&docs).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(beside.len(), 2, "{beside:?}");

        let broken = text.replace("Hello", "\\nosuchcommand Hello");
        let err = compile(&compiler, &src, &broken, &out, &base.join("tectonic"), None).unwrap_err();
        assert!(err.contains("Undefined control sequence"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn seed_copy_keeps_what_the_cache_has() {
        let base = std::env::temp_dir().join(format!("prisme-tex-seed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (seed, cache) = (base.join("seed"), base.join("cache"));
        std::fs::create_dir_all(seed.join("formats")).unwrap();
        std::fs::create_dir_all(seed.join("bundles/data")).unwrap();
        std::fs::write(seed.join("formats/x.fmt"), "fmt").unwrap();
        std::fs::write(seed.join("bundles/data/a.sty"), "seed").unwrap();
        std::fs::create_dir_all(cache.join("bundles/data")).unwrap();
        std::fs::write(cache.join("bundles/data/a.sty"), "newer").unwrap();
        assert!(has_format(&seed) && !has_format(&cache));

        copy_missing(&seed, &cache).unwrap();
        assert!(has_format(&cache));
        assert_eq!(std::fs::read_to_string(cache.join("bundles/data/a.sty")).unwrap(), "newer");
        let names: Vec<_> = std::fs::read_dir(cache.join("formats")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["x.fmt"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn search_path_keeps_defaults() {
        let p = search_path("PRISME_TEST_UNSET_VAR", Path::new("/docs"));
        let parts: Vec<PathBuf> = std::env::split_paths(&p).collect();
        assert_eq!(parts.first(), Some(&PathBuf::from("/docs")));
        assert_eq!(parts.last(), Some(&PathBuf::new()));
    }
}
