{
  description = "Tim Dev Environment";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };
  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
            clang
            gcc
            protobuf
            sccache
            buf
            rustToolchain
            nodejs_20
            (pkgs.writeShellScriptBin "codex" ''
              exec npx -y @openai/codex@latest "$@"
            '')
            (pkgs.writeShellScriptBin "claude" ''
              exec npx -y @anthropic-ai/claude-code "$@"
            '')
          ];
          buildInputs = with pkgs; [
            openssl
            libclang.lib
          ];
          
          RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          PROTOC = "${pkgs.protobuf}/bin/protoc";
          PROTOC_INCLUDE = "${pkgs.protobuf}/include";
          
          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
              pkgs.openssl
              pkgs.libclang.lib
            ]}:$LD_LIBRARY_PATH"
            
            export CC="sccache gcc"
            export CXX="sccache g++"
            export CMAKE_C_COMPILER="sccache gcc"
            export CMAKE_CXX_COMPILER="sccache g++"
            export CC_WRAPPER="sccache"
            
            echo "🚀 Go tim, go!"
            echo "Rust: $(rustc --version)"
            echo "Node: $(node --version)"
          '';
        };
      }
    );
}
