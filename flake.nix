{
  description = "Sophon speech-to-text service (NixOS)";

  inputs.nixpkgs.url = "nixpkgs";
  inputs.self.submodules = true;

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          runtimeDefaults = {
            pythonSupport = true;
            openvinoSupport = false;
          };
          cpuRuntime = pkgs.onnxruntime.override runtimeDefaults;
          cudaRuntime = pkgs.onnxruntime.override (runtimeDefaults // {
            cudaSupport = true;
          });
          migraphxRuntime = pkgs.onnxruntime.override (runtimeDefaults // {
            rocmSupport = true;
          });
          mkSophon = { name, onnxruntime, cargoFeatures ? [ ] }:
            pkgs.rustPlatform.buildRustPackage {
              pname = name;
              version = "0.1.0";
              src = self;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = [ "--bins" ]
                ++ pkgs.lib.optional (cargoFeatures != [ ]) "--features"
                ++ pkgs.lib.optional (cargoFeatures != [ ]) (pkgs.lib.concatStringsSep "," cargoFeatures);
              cargoInstallFlags = [ "--path" "." "--bins" ]
                ++ pkgs.lib.optional (cargoFeatures != [ ]) "--features"
                ++ pkgs.lib.optional (cargoFeatures != [ ]) (pkgs.lib.concatStringsSep "," cargoFeatures);
              cargoCheckType = "clippy";
              cargoCheckFlags = [ "--all-targets" "--" "-D" "warnings" ];

              nativeBuildInputs = [ pkgs.pkg-config pkgs.dbus ];
              buildInputs = [ pkgs.cacert pkgs.openssl onnxruntime pkgs.stdenv.cc.cc.lib ];
              ORT_LIB_LOCATION = "${onnxruntime}/lib";
              ORT_PREFER_DYNAMIC_LINK = "1";
              ORT_OFFLINE = "1";

              # nixpkgs provides the runtime. These settings prevent ort-sys
              # from downloading or statically embedding another copy.
              preCheck = ''
                export LD_LIBRARY_PATH=${onnxruntime}/lib:${pkgs.stdenv.cc.cc.lib}/lib
                export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt
                export SOPHON_DBUS_SESSION_CONFIG=${pkgs.dbus}/share/dbus-1/session.conf
              '';
              postInstall = ''
                mkdir -p $out/share/dbus-1/services
                cat > $out/share/dbus-1/services/com.garntresearch.sophon.service <<EOF
                [D-BUS Service]
                Name=com.garntresearch.sophon
                Exec=$out/bin/sophon
                EOF
              '';
              postFixup = ''
                for binary in $out/bin/*; do
                  patchelf --add-rpath ${onnxruntime}/lib:${pkgs.stdenv.cc.cc.lib}/lib "$binary"
                done
              '';
            };
        in rec {
          sophon-cpu = mkSophon {
            name = "sophon";
            onnxruntime = cpuRuntime;
          };
          sophon-cuda = mkSophon {
            name = "sophon-cuda";
            onnxruntime = cudaRuntime;
            cargoFeatures = [ "cuda" ];
          };
          sophon-migraphx = mkSophon {
            name = "sophon-migraphx";
            onnxruntime = migraphxRuntime;
            cargoFeatures = [ "migraphx" ];
          };
          default = sophon-cpu;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.sophon-cpu}/bin/sophon";
        };
      });

      devShells = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.pkg-config pkgs.openssl pkgs.dbus ];
          };
        });

      checks = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          cpuClosure = pkgs.closureInfo { rootPaths = [ self.packages.${system}.sophon-cpu ]; };
          cudaClosure = pkgs.closureInfo { rootPaths = [ self.packages.${system}.sophon-cuda ]; };
          migraphxClosure = pkgs.closureInfo { rootPaths = [ self.packages.${system}.sophon-migraphx ]; };
        in {
          cpu-provider = pkgs.runCommand "sophon-cpu-provider-smoke" {
            nativeBuildInputs = [ self.packages.${system}.sophon-cpu ];
          } ''
            cpu_provider_smoke
            touch $out
          '';
          migraphx-provider = pkgs.runCommand "sophon-migraphx-provider-smoke" {
            nativeBuildInputs = [ self.packages.${system}.sophon-migraphx ];
          } ''
            migraphx_provider_smoke
            touch $out
          '';
          fmt = pkgs.runCommand "sophon-format" { nativeBuildInputs = [ pkgs.rustfmt ]; } ''
            rustfmt --edition 2024 --check ${self}/src/*.rs
            touch $out
          '';
          dbus-activation = pkgs.runCommand "sophon-dbus-activation" {
            nativeBuildInputs = [ pkgs.dbus pkgs.systemd self.packages.${system}.sophon-cpu ];
          } ''
            mkdir -p config/sophon
            printf 'unknown: true\n' > config/sophon/config.yaml
            export XDG_CONFIG_HOME=$PWD/config
            export XDG_DATA_DIRS=${self.packages.${system}.sophon-cpu}/share
            cat > dbus-daemon <<'EOF'
            #!${pkgs.runtimeShell}
            args=()
            for arg in "$@"; do
              [ "$arg" = --session ] || args+=("$arg")
            done
            exec ${pkgs.dbus}/bin/dbus-daemon --config-file=${pkgs.dbus}/share/dbus-1/session.conf "''${args[@]}"
            EOF
            chmod +x dbus-daemon
            dbus-run-session --dbus-daemon=$PWD/dbus-daemon -- sh -c '
              dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply /org/freedesktop/DBus org.freedesktop.DBus.StartServiceByName string:com.garntresearch.sophon uint32:0 >/dev/null
              sleep 1
              busctl --user introspect com.garntresearch.sophon /com/garntresearch/sophon | grep -F TranscribeFile
              busctl --user get-property com.garntresearch.sophon /com/garntresearch/sophon com.garntresearch.sophon State | grep -F Failed
            '
            touch $out
          '';
          closure-policy = pkgs.runCommand "sophon-closure-policy" {} ''
            ! grep -Ei '/(cuda|rocm|migraphx|hip|wayland|xorg|gtk|qt|pulseaudio|pipewire|portal|alsa)-' ${cpuClosure}/store-paths
            ! grep -Ei '/(rocm|migraphx|amd|hip)-' ${cudaClosure}/store-paths
            grep -Ei '/(rocm|migraphx|hip)-' ${migraphxClosure}/store-paths
            touch $out
          '';
          cuda-evaluates = self.packages.${system}.sophon-cuda;
          migraphx-evaluates = self.packages.${system}.sophon-migraphx;
        });
    };
}
