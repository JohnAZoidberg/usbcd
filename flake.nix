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
          mkUsbc = { gui ? false }: pkgs.rustPlatform.buildRustPackage {
            pname = if gui then "usbc-gui" else "usbc";
            version = "0.1.0";
            src = ./.;
            cargoHash = "sha256-R5Zvq2kGS1rqUt5KQs5bBKZmR8eOt5jPV0kLMDHlS3Y=";
            buildFeatures = pkgs.lib.optional gui "gui";
            nativeBuildInputs = [ pkgs.pkg-config ]
              # glib-compile-resources for build.rs, plus the GTK wrapper.
              ++ pkgs.lib.optionals gui [ pkgs.glib pkgs.wrapGAppsHook4 ];
            buildInputs = [ pkgs.systemdMinimal ]
              ++ pkgs.lib.optionals gui [ pkgs.gtk4 pkgs.libadwaita ];
            postInstall = pkgs.lib.optionalString gui ''
              install -Dm644 usbc.desktop -t $out/share/applications
            '';
          };
        in
        {
          default = mkUsbc { };

          # Includes the `usbc gui` desktop window.
          gui = mkUsbc { gui = true; };
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
              pkgs.glib # glib-compile-resources, for build.rs
            ];
            buildInputs = [
              pkgs.systemdMinimal
              pkgs.gtk4
              pkgs.libadwaita
            ];
            # Binaries built here get no RUNPATH for these, so running
            # ./target/debug/usbc directly (not via cargo run) needs them.
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              pkgs.systemdMinimal
              pkgs.gtk4
              pkgs.libadwaita
              pkgs.pango
              pkgs.glib
            ];
          };
        });
    };
}
