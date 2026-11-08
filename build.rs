// Regenerate the icons from the repository root (requires librsvg's rsvg-convert):
// rsvg-convert -w 512 -h 512 assets/terminal.svg -o assets/terminal.png
// python3 <<'PY'
// import pathlib, struct, subprocess
// sizes = (16, 24, 32, 48, 64, 128, 256)
// images = [subprocess.check_output(['rsvg-convert', '-w', str(s), '-h', str(s),
//                                  'assets/terminal.svg']) for s in sizes]
// offset = 6 + 16 * len(sizes)
// entries = []
// for size, image in zip(sizes, images):
//     entries.append(struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0,
//                                1, 32, len(image), offset))
//     offset += len(image)
// pathlib.Path('assets/terminal.ico').write_bytes(
//     struct.pack('<HHH', 0, 1, len(sizes)) + b''.join(entries) + b''.join(images))
// PY
// macOS -> MSVC cross-builds need llvm-rc on PATH, e.g.:
// PATH="/opt/homebrew/opt/llvm/bin:$PATH" cargo xwin build --target x86_64-pc-windows-msvc
// Alternatively set RC_x86_64_pc_windows_msvc to the absolute llvm-rc path.

fn main() {
    println!("cargo:rerun-if-changed=assets/terminal.ico");
    println!("cargo:rerun-if-changed=assets/terminal.rc");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(
            "assets/terminal.rc",
            embed_resource::ParamsIncludeDirs(["assets"]),
        )
        .manifest_required()
        .expect("failed to embed the application icon");
    }
}
