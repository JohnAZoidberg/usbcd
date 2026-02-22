{
  description = "usbc - USB Type-C cable quality warning tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "usbc";
            version = "0.1.0";
            src = ./.;
            cargoHash = "sha256-KFq074YriiMAJ38khqIlxeK2ON9a/+2AjEUvnLQEwcg=";
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.systemdMinimal ];
          };
        });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = [
              pkgs.rustc
              pkgs.cargo
              pkgs.pkg-config
            ];
            buildInputs = [
              pkgs.systemdMinimal
            ];
          };
        });
    };
}
