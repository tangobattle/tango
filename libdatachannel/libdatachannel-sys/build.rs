fn rustc_link_search(cmake: &cmake::Config, path: &str) {
    let profile = cmake.get_profile();
    if cfg!(target_env = "msvc") {
        println!("cargo:rustc-link-search={path}/{profile}");
    } else {
        println!("cargo:rustc-link-search={path}")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::var("OUT_DIR").unwrap();

    let mut cmake = cmake::Config::new("libdatachannel");
    cmake.build_target("datachannel-static");
    cmake.out_dir(&out_dir);

    cmake.define("NO_WEBSOCKET", "ON");
    cmake.define("NO_EXAMPLES", "ON");
    cmake.define("NO_MEDIA", "ON");
    cmake.define("NO_TESTS", "ON");

    #[cfg(feature = "vendored")]
    {
        let openssl_artifacts = openssl_src::Build::new().build();
        cmake.define(
            "OPENSSL_ROOT_DIR",
            openssl_artifacts.lib_dir().parent().unwrap(),
        );
        cmake.define("OPENSSL_USE_STATIC_LIBS", "TRUE");

        println!(
            "cargo:rustc-link-search=native={}",
            openssl_artifacts.lib_dir().to_str().unwrap()
        );

        if cfg!(target_env = "msvc") {
            println!("cargo:rustc-link-lib=static=libcrypto");
            println!("cargo:rustc-link-lib=static=libssl");
        } else {
            println!("cargo:rustc-link-lib=static=crypto");
            println!("cargo:rustc-link-lib=static=ssl");
        }
    }

    #[cfg(not(feature = "vendored"))]
    {
        println!("cargo:rustc-link-lib=dylib=crypto");
        println!("cargo:rustc-link-lib=dylib=ssl");
    }

    cmake.build();

    cpp_build::Config::new()
        .include(format!("{}/lib", out_dir))
        .build("src/lib.rs");

    rustc_link_search(&cmake, &format!("native={out_dir}/build/deps/libjuice"));
    println!("cargo:rustc-link-lib=static=juice-static");

    rustc_link_search(
        &cmake,
        &format!("native={out_dir}/build/deps/usrsctp/usrsctplib"),
    );
    println!("cargo:rustc-link-lib=static=usrsctp");

    rustc_link_search(&cmake, &format!("native={out_dir}/build"));
    println!("cargo:rustc-link-lib=static=datachannel-static");

    let bindings = bindgen::Builder::default()
        .header("libdatachannel/include/rtc/rtc.h")
        .generate()?;

    let out_path = std::path::PathBuf::from(out_dir);
    bindings.write_to_file(out_path.join("bindings.rs"))?;

    Ok(())
}
