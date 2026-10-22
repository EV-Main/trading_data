use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use ui_test::{dependencies::DependencyBuilder, spanned::Spanned};

fn main() -> ui_test::color_eyre::Result<()> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));

	// Strict suite: every diagnostic is annotated inline (pins exact macro errors + the const
	// assert messages the macro emits — all stable strings).
	let mut strict = base_config(root.join("tests/compile_fail"), root);
	// Cascade suite: a wrong topo order fails the `Pull`/`Has` trait bounds, which the compiler
	// reports across every downstream site with unstable phrasing. Annotating each is brittle, so
	// this dir runs with annotations off and pins the full `.stderr` snapshot instead.
	let mut cascade = base_config(root.join("tests/compile_fail_cascade"), root);
	cascade.comment_defaults.base().require_annotations = Spanned::dummy(false).into();

	// A `.stderr` is one compiler's rendering of rustc's own snippets, which every nightly is free to
	// redraw; on any other compiler only the annotations and the failure itself are ours to hold.
	let blessed_with = root.join("tests/compile_fail.rustc");
	let rustc = commit_hash(&strict.program.program);
	if std::env::args().any(|a| a == "--bless") {
		fs::write(&blessed_with, format!("{rustc}\n"))?;
	}
	let blessed = fs::read_to_string(&blessed_with).expect("written by `--bless`");
	if blessed.trim() != rustc {
		eprintln!("compile_fail: snapshots were blessed with rustc {}, this is {rustc} — `.stderr` not compared", blessed.trim());
		strict.output_conflict_handling = ui_test::ignore_output_conflict;
		cascade.output_conflict_handling = ui_test::ignore_output_conflict;
	}

	ui_test::run_tests(strict)?;
	ui_test::run_tests(cascade)
}

fn commit_hash(rustc: &Path) -> String {
	let out = Command::new(rustc).arg("-vV").output().expect("the rustc ui_test drives runs");
	assert!(out.status.success(), "{} -vV failed", rustc.display());
	let info = String::from_utf8(out.stdout).expect("rustc -vV is utf-8");
	info.lines().find_map(|l| l.strip_prefix("commit-hash: ")).expect("rustc -vV names its commit").to_owned()
}

fn base_config(dir: PathBuf, root: &Path) -> ui_test::Config {
	let mut config = ui_test::Config::rustc(dir);
	// Resolve extern crates through cargo from a side manifest, so a case can reference both
	// `trading_data_macros` and `trading_data_dag` with correct features. Hand-picking rlibs can't.
	config.comment_defaults.base().set_custom(
		"dependencies",
		DependencyBuilder {
			crate_manifest_path: root.join("tests/compile_fail_deps/Cargo.toml"),
			..DependencyBuilder::default()
		},
	);
	config.bless_command = Some("cargo test --test compile_fail -- --bless".to_string());
	config.path_stderr_filter(root, "$DIR");
	// Snapshots pin diagnostic text, not source positions: an unrelated edit to a quoted file
	// otherwise shifts every line number, and with it the gutter width.
	config.stderr_filter(r"\.rs:\d+:\d+", ".rs:LL:CC");
	config.stderr_filter(r"(?m)^ *\d+( *\|)", "LL$1");
	// rustc spills an over-long type to a side file whose name carries a per-run hash.
	config.stderr_filter(r"long-type-\d+\.txt", "long-type.txt");
	config.stderr_filter(r"'[^']*/([^/']+\.long-type\.txt)'", "'$1'"); // lands under whichever target dir the run uses
	// A const-eval panic is reported against `core`'s own `panic.rs`, which is the toolchain's, not
	// ours: the sysroot path differs between rustup (`/rustc/<hash>`) and nix, and whether that frame
	// renders as a quoted source line or a bare note depends on `rust-src` being installed. Collapse
	// both spellings onto the note, so these snapshots pin our message and not the toolchain layout.
	config.stderr_filter(r"(?:/rustc/[0-9a-f]+|/\S+/lib/rustlib/src/rust)/library/", "<sysroot>/library/");
	config.stderr_filter(r"(?m)^LL \| *\$crate::panicking::panic_fmt.*\n", "");
	config.stderr_filter(r"(?m)^ *\| *\^+ (evaluation of .* failed here)$", "= note: $1");
	// rustc's own help around an unmet bound rewords between nightlies; the bound and our messages don't.
	config.stderr_filter(r"is conditionally implemented for", "is implemented for");
	config.stderr_filter(r"(?m)^.*unsatisfied requirement introduced here.*\n", "");
	config.stderr_filter(r"(?m)^ *= note: an? \w+ named `[^`]*` exists in another namespace\n", "");
	config.stderr_filter(r"((?:expected|found) struct `)(?:\w+::)+", "$1");
	config.stderr_filter(r"(?m)^(note: the above error was encountered while instantiating `fn [^`]*? as \w+)<[^`]*`", "$1<..>`");
	// Last, so it also squares the line above: rustc indents every gutter decoration to the width of
	// the widest line number in the snippet, and `LL` has already thrown those away. Two nightlies two
	// days apart disagreed by one space on one `= note:`, which is a red suite about nothing.
	config.stderr_filter(r"(?m)^ *(-->|\||:::|=)", "  $1");
	config
}
