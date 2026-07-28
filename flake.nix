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
          # CUDA package outputs require the redistributable CUDA toolkit.
          pkgs = import nixpkgs {
            inherit system;
            config.allowUnfree = true;
          };
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
          mkQwenttsCpp = {
            name,
            feature,
            nativeBuildInputs ? [ ],
            buildInputs ? [ ],
            runtimeLibraryPaths ? [ ],
            environment ? { },
          }:
            pkgs.rustPlatform.buildRustPackage (environment // {
              pname = name;
              version = "0.1.0";
              src = pkgs.lib.cleanSource ./.;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = [
                "-p" "qwentts-cpp" "--no-default-features" "--features" feature
              ];
              cargoTestFlags = [
                "-p" "qwentts-cpp" "--no-default-features" "--features" feature
              ];

              nativeBuildInputs = [
                pkgs.cmake
                pkgs.pkg-config
                pkgs.llvmPackages.libclang
              ] ++ nativeBuildInputs;
              buildInputs = [
                pkgs.openblas
                pkgs.stdenv.cc.cc.lib
              ] ++ buildInputs;
              propagatedBuildInputs = [ pkgs.openblas ] ++ runtimeLibraryPaths;
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              preCheck = ''
                export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath ([ pkgs.openblas pkgs.stdenv.cc.cc.lib ] ++ runtimeLibraryPaths)}
              '';
              installPhase = ''
                runHook preInstall
                rlib=$(find target -type f -name libqwentts_cpp.rlib -print -quit)
                nativeLibDir=$(find target -type d -path '*/build/qwentts-cpp-*/out/lib' -print -quit)
                test -n "$rlib"
                test -n "$nativeLibDir"
                install -Dm644 "$rlib" $out/lib/libqwentts_cpp.rlib
                find "$nativeLibDir" -maxdepth 1 -type f -name '*.so*' \
                  -exec install -Dm755 {} $out/lib/ \;
                test -f $out/lib/libqwen.so
                test ! -e $out/bin
                runHook postInstall
              '';
              preFixup = ''
                rpath="\$ORIGIN:${pkgs.lib.makeLibraryPath ([ pkgs.openblas pkgs.stdenv.cc.cc.lib ] ++ runtimeLibraryPaths)}"
                for library in $out/lib/*.so*; do
                  patchelf --set-rpath "$rpath" "$library"
                done
              '';
              passthru.backend = feature;
            });
          syclAvailable = pkgs ? intel-llvm && pkgs ? level-zero;
          syclUnavailable =
            "qwentts-cpp-sycl requires nixpkgs `intel-llvm` and `level-zero` packages; update nixpkgs to a revision providing an Intel-compatible -fsycl toolchain";
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
          qwentts-cpp-cpu = mkQwenttsCpp {
            name = "qwentts-cpp-cpu";
            feature = "cpu";
          };
          qwentts-cpp-cuda = mkQwenttsCpp {
            name = "qwentts-cpp-cuda";
            feature = "cuda";
            nativeBuildInputs = [ pkgs.cudaPackages.cuda_nvcc ];
            buildInputs = [ pkgs.cudaPackages.libcublas ];
            runtimeLibraryPaths = [ pkgs.cudaPackages.libcublas ];
            environment.CUDA_PATH = "${pkgs.cudaPackages.cuda_nvcc}";
          };
          qwentts-cpp-sycl =
            if syclAvailable then
              mkQwenttsCpp {
                name = "qwentts-cpp-sycl";
                feature = "sycl";
                nativeBuildInputs = [ pkgs.intel-llvm ];
                buildInputs = [ pkgs.level-zero pkgs.intel-compute-runtime ];
                runtimeLibraryPaths = [
                  pkgs.intel-llvm
                  pkgs.level-zero
                  pkgs.intel-compute-runtime
                ];
                environment = {
                  CXX = "${pkgs.intel-llvm}/bin/clang++";
                  ONEAPI_ROOT = "${pkgs.intel-llvm}";
                };
              }
            else throw syclUnavailable;
          qwentts-cpp-vulkan = mkQwenttsCpp {
            name = "qwentts-cpp-vulkan";
            feature = "vulkan";
            nativeBuildInputs = [ pkgs.shaderc ];
            buildInputs = [
              pkgs.spirv-headers
              pkgs.vulkan-headers
              pkgs.vulkan-loader
            ];
            runtimeLibraryPaths = [ pkgs.vulkan-loader ];
          };
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
          qwenttsCpu = self.packages.${system}.qwentts-cpp-cpu;
          qwenttsVulkan = self.packages.${system}.qwentts-cpp-vulkan;
          qwenttsCpuClosure = pkgs.closureInfo { rootPaths = [ qwenttsCpu ]; };
          qwenttsVulkanClosure = pkgs.closureInfo { rootPaths = [ qwenttsVulkan ]; };
          qwenttsCudaEvaluates = builtins.tryEval self.packages.${system}.qwentts-cpp-cuda.drvPath;
          qwenttsSyclEvaluates = builtins.tryEval self.packages.${system}.qwentts-cpp-sycl.drvPath;
        in {
          qwentts-cpp-cpu-runtime = pkgs.runCommand "qwentts-cpp-cpu-runtime" {} ''
            test -f ${qwenttsCpu}/lib/libqwen.so
            test -f ${qwenttsCpu}/lib/libggml.so
            test -f ${qwenttsCpu}/lib/libggml-base.so
            test -f ${qwenttsCpu}/lib/libggml-blas.so
            test -f ${qwenttsCpu}/lib/libggml-cpu.so
            test ! -e ${qwenttsCpu}/bin
            grep -F openblas ${qwenttsCpuClosure}/store-paths
            ! grep -Ei -- '-(cuda|vulkan|sycl|oneapi|intel-llvm|level-zero)-' ${qwenttsCpuClosure}/store-paths
            touch $out
          '';
          qwentts-cpp-vulkan-runtime = pkgs.runCommand "qwentts-cpp-vulkan-runtime" {} ''
            test -f ${qwenttsVulkan}/lib/libqwen.so
            test -f ${qwenttsVulkan}/lib/libggml.so
            test -f ${qwenttsVulkan}/lib/libggml-base.so
            test -f ${qwenttsVulkan}/lib/libggml-blas.so
            test -f ${qwenttsVulkan}/lib/libggml-cpu.so
            test -f ${qwenttsVulkan}/lib/libggml-vulkan.so
            test ! -e ${qwenttsVulkan}/bin
            grep -F openblas ${qwenttsVulkanClosure}/store-paths
            grep -F vulkan-loader ${qwenttsVulkanClosure}/store-paths
            ! grep -Ei -- '-(cuda|sycl|oneapi|intel-llvm|level-zero)-' ${qwenttsVulkanClosure}/store-paths
            touch $out
          '';
          qwentts-cpp-cuda-evaluates = assert qwenttsCudaEvaluates.success;
            pkgs.runCommand "qwentts-cpp-cuda-evaluates" {} "touch $out";
          qwentts-cpp-sycl-evaluates = assert qwenttsSyclEvaluates.success;
            pkgs.runCommand "qwentts-cpp-sycl-evaluates" {} "touch $out";
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
            rustfmt --edition 2024 --check ${self}/src/*.rs ${self}/qwentts-cpp/src/*.rs ${self}/qwentts-cpp/*.rs ${self}/qwentts-cpp/tests/*.rs
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
