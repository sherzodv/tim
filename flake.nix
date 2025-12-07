{
  description = "Tim Dev Environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
            clang
            protobuf
            sccache
            buf
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
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
            libclang
          ];

          RUSTC_WRAPPER = "sccache";
          CMAKE_C_COMPILER_LAUNCHER = "sccache";
          CMAKE_CXX_COMPILER_LAUNCHER = "sccache";

          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          PROTOC = "${pkgs.protobuf}/bin/protoc";
          PROTOC_INCLUDE = "${pkgs.protobuf}/include";

          # Environment variables
          shellHook = ''
            export CC='sccache gcc'
            export CXX='sccache g++'
            export CMAKE_C_COMPILER='sccache gcc'
            export CMAKE_CXX_COMPILER='sccache g++'
            echo "🚀 Go tim, go!"
            echo "Rust: $(rustc --version)"
            echo "Node: $(node --version)"
          '';
        };
      }
    );
}
