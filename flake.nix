{
  description = "A minimal GBA emulator written in Rust";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
    in {
      devShells = nixpkgs.lib.genAttrs systems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell ({
            packages = with pkgs; [ rustc cargo rustfmt clippy ];
          } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
            # minifb 0.28 hard-codes macOS 10.10 before user CFLAGS.
            # Match the Nix SDK's deployment target so Metal APIs are available.
            # Its Objective-C files also share a tentative g_metal_device definition.
            CFLAGS = "-mmacosx-version-min=${pkgs.stdenv.hostPlatform.darwinMinVersion} -fcommon";
          });
        });
    };
}
