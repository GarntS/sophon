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
              version = "2026.1.0";
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
          mkSophon = { name, onnxruntime, qwenBackend, cargoFeatures ? [ ] }:
            let
              featureFlags = [ "--no-default-features" ]
                ++ pkgs.lib.optional (cargoFeatures != [ ]) "--features"
                ++ pkgs.lib.optional (cargoFeatures != [ ]) (pkgs.lib.concatStringsSep "," cargoFeatures);
              qwenNativeBuildInputs =
                pkgs.lib.optionals (qwenBackend == "cuda") [ pkgs.cudaPackages.cuda_nvcc ]
                ++ pkgs.lib.optionals (qwenBackend == "vulkan") [ pkgs.shaderc ];
              qwenBuildInputs = [ pkgs.openblas pkgs.stdenv.cc.cc.lib ]
                ++ pkgs.lib.optionals (qwenBackend == "cuda") [ pkgs.cudaPackages.libcublas ]
                ++ pkgs.lib.optionals (qwenBackend == "vulkan") [
                  pkgs.spirv-headers
                  pkgs.vulkan-headers
                  pkgs.vulkan-loader
                ];
              qwenRuntimeLibraries =
                pkgs.lib.optionals (qwenBackend == "cuda") [ pkgs.cudaPackages.libcublas ]
                ++ pkgs.lib.optionals (qwenBackend == "vulkan") [ pkgs.vulkan-loader ];
            in pkgs.rustPlatform.buildRustPackage {
              pname = name;
              version = "2026.2.1";
              src = self;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = [ "--bins" ] ++ featureFlags;
              cargoInstallFlags = [ "--path" "." "--bins" ] ++ featureFlags;
              cargoCheckType = "clippy";
              cargoCheckFlags = [ "--all-targets" ] ++ featureFlags ++ [ "--" "-D" "warnings" ];
              cargoTestFlags = featureFlags;

              nativeBuildInputs = [
                pkgs.cmake
                pkgs.pkg-config
                pkgs.dbus
                pkgs.makeWrapper
                pkgs.llvmPackages.libclang
              ] ++ qwenNativeBuildInputs;
              buildInputs = [
                pkgs.alsa-lib
                pkgs.cacert
                pkgs.openssl
                pkgs.pipewire
                onnxruntime
              ] ++ qwenBuildInputs;
              BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.glibc.dev}/include";
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              ORT_LIB_LOCATION = "${onnxruntime}/lib";
              ORT_PREFER_DYNAMIC_LINK = "1";
              ORT_OFFLINE = "1";
              CUDA_PATH = if qwenBackend == "cuda" then "${pkgs.cudaPackages.cuda_nvcc}" else "";

              # nixpkgs provides the runtime. These settings prevent ort-sys
              # from downloading or statically embedding another copy.
              preCheck = ''
                export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath ([ onnxruntime pkgs.openblas pkgs.stdenv.cc.cc.lib ] ++ qwenRuntimeLibraries)}
                export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt
                export SOPHON_DBUS_SESSION_CONFIG=${pkgs.dbus}/share/dbus-1/session.conf
              '';
              postInstall = ''
                nativeLibDir=$(find target -type d -path '*/build/qwentts-cpp-*/out/lib' -print -quit)
                test -n "$nativeLibDir"
                mkdir -p $out/lib
                find "$nativeLibDir" -maxdepth 1 -type f -name '*.so*' \
                  -exec install -Dm755 {} $out/lib/ \;
                test -f $out/lib/libqwen.so
                test -f $out/lib/libggml.so
                test -f $out/lib/libggml-base.so
                test -f $out/lib/libggml-${qwenBackend}.so || test "${qwenBackend}" = cpu
                test -f $out/lib/libggml-cpu.so || test "${qwenBackend}" != cpu

                install -Dm444 model_registry.yaml $out/share/sophon/model_registry.yaml

                mkdir -p $out/share/dbus-1/services
                cat > $out/share/dbus-1/services/com.garntresearch.sophon.service <<EOF
                [D-BUS Service]
                Name=com.garntresearch.sophon
                Exec=$out/bin/sophon
                EOF
              '';
              preFixup = ''
                externalRpath=${pkgs.lib.makeLibraryPath ([ onnxruntime pkgs.alsa-lib pkgs.pipewire pkgs.openblas pkgs.stdenv.cc.cc.lib ] ++ qwenRuntimeLibraries)}
                for library in $out/lib/*.so*; do
                  patchelf --set-rpath "\$ORIGIN:$externalRpath" "$library"
                done
                for binary in $out/bin/*; do
                  patchelf --add-rpath "\$ORIGIN/../lib:$externalRpath" "$binary"
                done
                wrapProgram $out/bin/sophon \
                  --set SOPHON_MODEL_REGISTRY_PATH $out/share/sophon/model_registry.yaml \
                  --set SSL_CERT_FILE ${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt \
                  --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.espeak-ng ]}
              '';
              passthru.qwenBackend = qwenBackend;
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
            qwenBackend = "cpu";
            cargoFeatures = [ "qwen-cpu" ];
          };
          sophon-cuda = mkSophon {
            name = "sophon-cuda";
            onnxruntime = cudaRuntime;
            qwenBackend = "cuda";
            cargoFeatures = [ "cuda" ];
          };
          sophon-migraphx = mkSophon {
            name = "sophon-migraphx";
            onnxruntime = migraphxRuntime;
            qwenBackend = "vulkan";
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
        let
          pkgs = import nixpkgs { inherit system; };
          onnxruntime = pkgs.onnxruntime.override {
            pythonSupport = true;
            openvinoSupport = false;
          };
        in {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.cmake
              pkgs.pkg-config
              pkgs.alsa-lib
              pkgs.openblas
              pkgs.stdenv.cc
              pkgs.openssl
              pkgs.dbus
              pkgs.pipewire
              pkgs.espeak-ng
              pkgs.llvmPackages.libclang
              onnxruntime
            ];
            BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.glibc.dev}/include";
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ onnxruntime pkgs.openblas pkgs.stdenv.cc.cc.lib ];
            ORT_LIB_LOCATION = "${onnxruntime}/lib";
            ORT_PREFER_DYNAMIC_LINK = "1";
            ORT_OFFLINE = "1";
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
          mkSophonQwenRuntimeCheck = { name, package, backend, forbiddenBackends }:
            pkgs.runCommand name { nativeBuildInputs = [ pkgs.binutils pkgs.patchelf ]; } ''
              for library in libqwen.so libggml.so libggml-base.so libggml-blas.so libggml-${backend}.so; do
                test -f ${package}/lib/$library
              done
              for forbidden in ${pkgs.lib.concatStringsSep " " forbiddenBackends}; do
                test ! -e ${package}/lib/libggml-$forbidden.so
              done
              patchelf --print-rpath ${package}/bin/.sophon-wrapped | grep -F '$ORIGIN/../lib'
              patchelf --print-rpath ${package}/lib/libqwen.so | grep -F '$ORIGIN'
              ! ldd ${package}/bin/.sophon-wrapped | grep -F 'not found'
              ldd ${package}/bin/.sophon-wrapped | grep -F libqwen.so
              readelf -d ${package}/lib/libqwen.so | grep -F 'libggml-${backend}.so'
              touch $out
            '';
        in {
          model-registry = pkgs.runCommand "sophon-model-registry" { nativeBuildInputs = [ pkgs.yq ]; } ''
            registry=${self.packages.${system}.sophon-cpu}/share/sophon/model_registry.yaml
            test -f "$registry"
            test "$(stat -c %a "$registry")" = 444
            yq -e '.providers."transcribe-rs"."parakeet-tdt-0.6b-v3-int8".files.encoder.sha256 == "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09"' "$registry"
            yq -e '.providers."qwentts-cpp" | length == 5' "$registry"
            yq -e '[.providers."qwentts-cpp"[].files.codec.sha256] | unique | length == 1' "$registry"
            touch $out
          '';
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
          sophon-cpu-qwen-runtime = mkSophonQwenRuntimeCheck {
            name = "sophon-cpu-qwen-runtime";
            package = self.packages.${system}.sophon-cpu;
            backend = "cpu";
            forbiddenBackends = [ "cuda" "vulkan" ];
          };
          sophon-cuda-qwen-runtime = mkSophonQwenRuntimeCheck {
            name = "sophon-cuda-qwen-runtime";
            package = self.packages.${system}.sophon-cuda;
            backend = "cuda";
            forbiddenBackends = [ "vulkan" ];
          };
          sophon-migraphx-qwen-runtime = mkSophonQwenRuntimeCheck {
            name = "sophon-migraphx-qwen-runtime";
            package = self.packages.${system}.sophon-migraphx;
            backend = "vulkan";
            forbiddenBackends = [ "cuda" ];
          };
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
            rustfmt --edition 2024 --check ${self}/src/*.rs ${self}/crates/qwentts-cpp/src/*.rs ${self}/crates/qwentts-cpp/*.rs ${self}/crates/qwentts-cpp/tests/*.rs
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
            grep -F 'name = "tts-rs"' ${self}/Cargo.lock
            for closure in ${cpuClosure} ${cudaClosure} ${migraphxClosure}; do
              grep -Ei -- '-pipewire-' "$closure/store-paths"
              grep -Ei -- '-espeak-ng-' "$closure/store-paths"
              grep -Ei -- '-onnxruntime-' "$closure/store-paths"
            done
            ! grep -Ei -- '-(cuda|vulkan|rocm|migraphx|hip|xorg|gtk|qt|pulseaudio|portal)-' ${cpuClosure}/store-paths
            grep -Ei -- '-cuda-' ${cudaClosure}/store-paths
            ! grep -Ei -- '-(vulkan|rocm|migraphx|amd|hip)-' ${cudaClosure}/store-paths
            grep -Ei -- '-(rocm|migraphx|hip)-' ${migraphxClosure}/store-paths
            grep -Ei -- '-vulkan-loader-' ${migraphxClosure}/store-paths
            ! grep -Ei -- '-cuda-' ${migraphxClosure}/store-paths
            touch $out
          '';
          cuda-evaluates = self.packages.${system}.sophon-cuda;
          migraphx-evaluates = self.packages.${system}.sophon-migraphx;
        });
    };
}
