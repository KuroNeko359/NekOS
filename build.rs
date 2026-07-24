use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::fs;

fn compile_asm(src: &PathBuf, obj: &PathBuf) {
    let status = Command::new("riscv64-elf-gcc")
        .args(&[
            "-c",
            "-march=rv64gc",
            "-mabi=lp64d",
            "-o",
            obj.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to compile assembly");

    assert!(status.success(), "{} compilation failed", src.display());
}

fn archive_obj(obj: &PathBuf, archive: &PathBuf) {
    let status = Command::new("riscv64-elf-ar")
        .args(&["crs", archive.to_str().unwrap(), obj.to_str().unwrap()])
        .status()
        .expect("Failed to archive assembly object");

    assert!(status.success(), "{} archive failed", archive.display());
}

fn compile_userlib(src: &PathBuf, rlib: &PathBuf) {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .arg("--edition=2021")
        .arg("--crate-name=userlib")
        .arg("--crate-type=rlib")
        .arg("--target=riscv64gc-unknown-none-elf")
        .arg("-Copt-level=z")
        .arg("-Cpanic=abort")
        .arg("-o")
        .arg(rlib)
        .arg(src)
        .status()
        .expect("Failed to compile userlib");

    assert!(status.success(), "{} compilation failed", src.display());
}

fn compile_rust_user(
    src: &PathBuf,
    elf: &PathBuf,
    linker_script: &PathBuf,
    userlib: &PathBuf,
) {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .arg("--edition=2021")
        .arg("--crate-type=bin")
        .arg("--target=riscv64gc-unknown-none-elf")
        .arg("--extern")
        .arg(format!("userlib={}", userlib.display()))
        .arg("-Copt-level=z")
        .arg("-Cpanic=abort")
        .arg("-Crelocation-model=static")
        .arg("-Cstrip=symbols")
        .arg("-Clink-arg=--gc-sections")
        .arg(format!("-Clink-arg=-T{}", linker_script.display()))
        .arg("-o")
        .arg(elf)
        .arg(src)
        .status()
        .expect("Failed to compile Rust user program");

    assert!(status.success(), "{} compilation failed", src.display());
}

fn compile_c_user(
    src: &PathBuf,
    elf: &PathBuf,
    linker_script: &PathBuf,
    include_dir: &PathBuf,
    runtime: &PathBuf,
    posix: &PathBuf,
    stdio: &PathBuf,
    alloc: &PathBuf,
    startup: &PathBuf,
) {
    let status = Command::new("riscv64-elf-gcc")
        .args([
            "-march=rv64gc",
            "-mabi=lp64d",
            "-mcmodel=medany",
            "-msmall-data-limit=0",
            "-O2",
            "-ffreestanding",
            "-fno-builtin",
            "-fno-stack-protector",
            "-ffunction-sections",
            "-fdata-sections",
            "-fno-pic",
            "-fno-pie",
            "-nostdlib",
            "-nostartfiles",
            "-no-pie",
            "-Wl,--gc-sections",
            "-Wl,--build-id=none",
            "-s",
            "-I",
        ])
        .arg(include_dir)
        .arg("-T")
        .arg(linker_script)
        .arg("-o")
        .arg(elf)
        .arg(startup)
        .arg(runtime)
        .arg(posix)
        .arg(stdio)
        .arg(alloc)
        .arg(src)
        .status()
        .expect("Failed to compile C user program");

    assert!(status.success(), "{} compilation failed", src.display());
}

fn append_newc_entry(archive: &mut Vec<u8>, name: &str, data: &[u8], ino: u32) {
    let namesize = name.len() + 1;
    let header = format!(
        "070701{ino:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{namesize:08x}{:08x}",
        0o100755u32, 0, 0, 1, 0, data.len(), 0, 0, 0, 0, 0
    );
    assert_eq!(header.len(), 110);
    archive.extend_from_slice(header.as_bytes());
    archive.extend_from_slice(name.as_bytes());
    archive.push(0);
    while archive.len() % 4 != 0 { archive.push(0); }
    archive.extend_from_slice(data);
    while archive.len() % 4 != 0 { archive.push(0); }
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let src_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    
    // 编译汇编启动代码
    let start_asm = src_dir.join("src").join("arch").join("riscv").join("start.S");
    let trap_asm = src_dir.join("src").join("arch").join("riscv").join("trap.S");
    let user_asm = src_dir.join("src").join("arch").join("riscv").join("user.S");
    
    let start_obj = out_dir.join("start.o");
    let trap_obj = out_dir.join("trap.o");
    let user_obj = out_dir.join("user.o");
    let start_lib = out_dir.join("libstart.a");
    let trap_lib = out_dir.join("libtrap.a");
    let user_lib = out_dir.join("libuser.a");
    
    compile_asm(&start_asm, &start_obj);
    compile_asm(&trap_asm, &trap_obj);
    compile_asm(&user_asm, &user_obj);

    archive_obj(&start_obj, &start_lib);
    archive_obj(&trap_obj, &trap_lib);
    archive_obj(&user_obj, &user_lib);

    // 构建 userlib 和独立用户 ELF，并打包成内嵌 newc initrd。
    let programs_dir = src_dir.join("programs");
    let user_linker_script = programs_dir.join("user.ld");
    let user_dir = src_dir.join("user");
    let userlib = out_dir.join("libuserlib.rlib");
    compile_userlib(&user_dir.join("userlib/src/lib.rs"), &userlib);
    let c_include = user_dir.join("include");
    let c_runtime = user_dir.join("libc/nekos.c");
    let c_posix = user_dir.join("libc/posix.c");
    let c_stdio = user_dir.join("libc/printf.c");
    let c_alloc = user_dir.join("libc/malloc.c");
    let c_startup = user_dir.join("libc/crt0.S");

    let user_programs_dir = user_dir.join("programs");
    let mut user_programs: Vec<String> = fs::read_dir(&user_programs_dir)
        .expect("Failed to read user programs directory")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            let program = user_programs_dir.join(name);
            program.join("src/main.rs").is_file()
                || program.join("src/main.c").is_file()
        })
        .collect();
    user_programs.sort();

    let mut user_elfs = Vec::new();
    for name in user_programs {
        let program = user_programs_dir.join(&name);
        let rust_source = program.join("src/main.rs");
        let c_source = program.join("src/main.c");
        let elf = out_dir.join(&name);
        if rust_source.is_file() {
            compile_rust_user(&rust_source, &elf, &user_linker_script, &userlib);
        } else if c_source.is_file() {
            compile_c_user(
                &c_source,
                &elf,
                &user_linker_script,
                &c_include,
                &c_runtime,
                &c_posix,
                &c_stdio,
                &c_alloc,
                &c_startup,
            );
        } else {
            unreachable!();
        }
        user_elfs.push((name, elf));
    }

    let mut cpio = Vec::new();
    for (index, (name, elf)) in user_elfs.iter().enumerate() {
        append_newc_entry(
            &mut cpio,
            name.as_str(),
            &fs::read(elf).unwrap(),
            index as u32 + 1,
        );
    }
    append_newc_entry(&mut cpio, "TRAILER!!!", &[], user_elfs.len() as u32 + 1);
    let initrd_bin = out_dir.join("rootfs.cpio");
    fs::write(&initrd_bin, cpio).unwrap();
    let initrd_bin_obj = out_dir.join("initrd-bin.o");
    let status = Command::new("riscv64-elf-ld")
        .current_dir(&out_dir)
        .args(["-r", "-b", "binary", "-o", "initrd-bin.o", "rootfs.cpio"])
        .status()
        .expect("Failed to embed initrd");
    assert!(status.success(), "initrd embedding failed");
    let initrd_abi_obj = out_dir.join("initrd-abi.o");
    compile_asm(&src_dir.join("src/arch/riscv/initrd.S"), &initrd_abi_obj);
    let initrd_obj = out_dir.join("initrd.o");
    let status = Command::new("riscv64-elf-ld")
        .args(["--no-warn-mismatch", "-r", "-o"])
        .arg(&initrd_obj)
        .arg(&initrd_abi_obj)
        .arg(&initrd_bin_obj)
        .status()
        .expect("Failed to tag initrd ABI");
    assert!(status.success(), "initrd ABI link failed");
    let initrd_lib = out_dir.join("libinitrd.a");
    archive_obj(&initrd_obj, &initrd_lib);
    
    // 告诉cargo链接这些目标文件
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static:+whole-archive=start");
    println!("cargo:rustc-link-lib=static:+whole-archive=trap");
    println!("cargo:rustc-link-lib=static:+whole-archive=user");
    println!("cargo:rustc-link-lib=static:+whole-archive=initrd");
    println!("cargo:rustc-link-arg=-T{}", src_dir.join("linker.ld").display());
    
    // 重新编译条件
    println!("cargo:rerun-if-changed=src/arch/riscv/start.S");
    println!("cargo:rerun-if-changed=src/arch/riscv/trap.S");
    println!("cargo:rerun-if-changed=src/arch/riscv/user.S");
    println!("cargo:rerun-if-changed=programs/user.ld");
    println!("cargo:rerun-if-changed=user/userlib/src");
    println!("cargo:rerun-if-changed=user/include");
    println!("cargo:rerun-if-changed=user/libc");
    println!("cargo:rerun-if-changed=user/programs");
    println!("cargo:rerun-if-changed=src/arch/riscv/initrd.S");
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=build.rs");
}
