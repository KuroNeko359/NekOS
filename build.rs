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

    // 构建一个独立用户 ELF，并打包成内嵌 newc initrd。
    let programs_dir = src_dir.join("programs");
    let hello_obj = out_dir.join("hello.o");
    let hello_elf = out_dir.join("hello");
    compile_asm(&programs_dir.join("hello.S"), &hello_obj);
    let status = Command::new("riscv64-elf-ld")
        .args(["-T", programs_dir.join("user.ld").to_str().unwrap(), "-o"])
        .arg(&hello_elf)
        .arg(&hello_obj)
        .status()
        .expect("Failed to link user program");
    assert!(status.success(), "user program link failed");

    let mut cpio = Vec::new();
    append_newc_entry(&mut cpio, "hello", &fs::read(&hello_elf).unwrap(), 1);
    append_newc_entry(&mut cpio, "TRAILER!!!", &[], 2);
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
    println!("cargo:rerun-if-changed=programs/hello.S");
    println!("cargo:rerun-if-changed=programs/user.ld");
    println!("cargo:rerun-if-changed=src/arch/riscv/initrd.S");
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=build.rs");
}
