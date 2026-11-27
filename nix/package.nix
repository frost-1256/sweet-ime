{
  lib,
  stdenv,
  pkgs,
  rustPlatform,
  cargo,
  rustc,
  cmake,
  pkg-config,
  makeWrapper,
  fcitx5,
  cairo,
  pango,
  wayland,
  wayland-scanner,
  xorg,
  fetchFromGitHub,
  fetchurl,
  unzip,
  fontconfig,
  curl,
  dbus,
  xdg-utils,
}:

let
  # Must match the revision used by CI (.github/workflows/windows.yml).
  mozc = fetchFromGitHub {
    owner = "google";
    repo = "mozc";
    rev = "1527a4cd1217cec4611cd4bc8d46f8c66ea79e6a";
    sparseCheckout = [ "src/data/dictionary_oss" ];
    hash = "sha256-mVCR2y4YQSpYNOk1NQxl/HVGD5vMqNx1fDG2lIOJpqg=";
  };
  # Keep the version and hashes in step with scripts/fetch-sudachidict.ps1.
  sudachiVersion = "20261015";
  sudachiLexicons = lib.mapAttrsToList (name: hash: fetchurl {
    inherit hash;
    url = "https://sudachi.s3.ap-northeast-1.amazonaws.com/sudachidict-raw/v1/${sudachiVersion}/${name}.zip";
  }) {
    small_lex = "sha256-mBt4zcUyzgWLNMds2azTyabHccwM5SjX3pJCSyEXISU=";
    core_lex = "sha256-6awGIi8M5AGg/vmxKeYek9WeP/OVobA39JiWLuW4rYs=";
    notcore_lex = "sha256-FgctdavmoEiUeqy87P4atlogCWWp8d23zqs2Y0j6IfI=";
  };
  guiLibs = import ./gui-libs.nix pkgs;
in
stdenv.mkDerivation (finalAttrs: {
  pname = "sweet-ime";
  version = "0.8.7";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../assets/icon
      ../assets/licenses
      ../scripts/get-zenz.sh
      ../LICENSE
    ];
  };

  cargoDeps = rustPlatform.importCargoLock { lockFile = ../Cargo.lock; };

  nativeBuildInputs = [
    cmake
    pkg-config
    cargo
    rustc
    rustPlatform.cargoSetupHook
    makeWrapper
    unzip
    wayland-scanner
  ];
  buildInputs = [
    fcitx5
    cairo
    pango
    wayland
    xorg.libxcb
    xorg.xcbutilwm
  ];

  cmakeDir = "../crates/fcitx5";
  cmakeFlags = [
    (lib.cmakeFeature "SWEET_DATADIR" "${placeholder "out"}/share/sweet-ime")
    (lib.cmakeFeature "SWEET_SETTINGS" "${placeholder "out"}/bin/sweet-settings")
  ];

  # One cargo target directory for the addon (built by CMake) and the tools.
  preConfigure = ''
    export CARGO_TARGET_DIR="$NIX_BUILD_TOP/cargo-target"
    export SWEET_IME_CURL="${curl}/bin/curl"
  '';

  postBuild = ''
    (cd .. && cargo build --release --locked --offline \
      --bin sweet-dictc --bin sweet-settings --bin sweet-zenz-pack)
    sudachi_dir="$NIX_BUILD_TOP/sudachidict"
    mkdir -p "$sudachi_dir"
    for zip in ${lib.escapeShellArgs (map builtins.toString sudachiLexicons)}; do
      unzip -q "$zip" -d "$sudachi_dir"
    done
    "$CARGO_TARGET_DIR/release/sweet-dictc" ${mozc}/src/data/dictionary_oss system.dict "$sudachi_dir"
  '';

  postInstall = ''
    install -Dm644 system.dict "$out/share/sweet-ime/system.dict"
    install -Dm755 "$CARGO_TARGET_DIR/release/sweet-settings" "$out/bin/sweet-settings"
    "$out/bin/sweet-settings" --write-fcitx-themes "$out/share/fcitx5/themes"
    install -Dm755 "$CARGO_TARGET_DIR/release/sweet-zenz-pack" "$out/bin/sweet-zenz-pack"
    install -Dm644 ../crates/settings/sweet-settings.desktop "$out/share/applications/sweet-settings.desktop"
    substituteInPlace "$out/share/applications/sweet-settings.desktop" \
      --replace-fail "Exec=sweet-settings" "Exec=$out/bin/sweet-settings"
    install -Dm755 ../scripts/get-zenz.sh "$out/share/sweet-ime/get-zenz.sh"
    install -Dm644 ../LICENSE "$out/share/sweet-ime/licenses/sweet-ime.txt"
    install -Dm644 ../assets/licenses/english-words.txt "$out/share/sweet-ime/licenses/english-words.txt"
    install -Dm644 ../assets/licenses/sudachidict.txt "$out/share/sweet-ime/licenses/sudachidict.txt"
    install -Dm644 ../assets/licenses/wayland-protocols.txt "$out/share/sweet-ime/licenses/wayland-protocols.txt"
    install -Dm644 ${mozc}/src/data/dictionary_oss/README.txt "$out/share/sweet-ime/licenses/dictionary-oss.txt"
    wrapProgram "$out/bin/sweet-settings" \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath guiLibs} \
      --prefix PATH : ${lib.makeBinPath [ fontconfig curl dbus xdg-utils ]}
  '';

  meta = {
    description = "Japanese input method with a dictionary lattice and optional neural reranking (fcitx5 addon)";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "sweet-settings";
  };
})
