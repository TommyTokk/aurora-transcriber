# Milestone 1 — Audit di compatibilità Windows x64

Report del Milestone 1 del piano _AURORA — Windows Build & Distribution Plan_.
È un audit: **nessun file di codice, configurazione, risorsa o lockfile è stato
modificato**. L'unico file aggiunto è questo documento.

## 1. Contesto e metodo

| Voce                 | Valore                                                                                  |
| -------------------- | --------------------------------------------------------------------------------------- |
| Repository           | `Dunixx/aurora-transcriber`, branch `milestone-1-windows-audit`                         |
| Commit di partenza   | `304be7c` (`main`), albero pulito prima dell'audit                                      |
| Ambiente dell'audit  | Container Linux x86_64. **Nessun Windows, MSVC, Vulkan SDK o GPU disponibile**          |
| Istruzioni applicate | `AGENTS.md` e `message.md` forniti nell'archivio del progetto (entrambi git-ignorati)   |
| Fonti upstream       | Sorgenti dei crate scaricati con `cargo fetch --locked`, release GitHub, clone di Handy |

Legenda usata nel documento:

- **[V]** verificato leggendo il sorgente o ispezionando l'artefatto ufficiale;
- **[W]** dedotto dal sorgente, **da verificare su Windows reale**;
- **[D]** decisione aperta, da prendere prima della milestone indicata.

Riferimenti esterni esaminati:

- Handy, clone del commit `5a9451e70aedc90648ce2befd9d7e0304746f9c1` (21/10/2026).
- ONNX Runtime 1.20.1 `onnxruntime-win-x64-1.20.1.zip`, scaricato dalla release ufficiale:
  65.585.496 byte, SHA-256 calcolato
  `78d447051e48bd2e1e778bba378bec4ece11191c9e538cf7b2c4a4565e8f5581`.
  La pagina della release **non pubblica digest** per questo asset.
- transcribe.cpp `transcribe-native-0.2.4-windows-x86_64-cpu-vulkan.tar.gz`, SHA-256
  `09705f54218817c065602ada8fd0f4d13b3f7fbb9d94929eeb3789c6c2b1f34a`, che coincide
  con il digest mostrato dalla release.

## 2. Versioni effettive

Le versioni risolte coincidono con quelle indicate nel piano; non ci sono differenze da documentare. **[V]**

| Componente                              | Piano      | Repository (lockfile / manifest)                                 |
| --------------------------------------- | ---------- | ---------------------------------------------------------------- |
| ONNX Runtime                            | 1.20.1     | 1.20.1 (`resources/onnxruntime/manifest.json`, solo macOS arm64) |
| `ort` / `ort-sys`                       | 2.0.0-rc.9 | `=2.0.0-rc.9`, `default-features = false`, `load-dynamic`        |
| `transcribe-cpp` / `-sys`               | 0.2.4      | `=0.2.4`, `default-features = false`                             |
| `tauri` / `tauri-build` / `tauri-utils` | —          | 2.12.2 / 2.7.1 / 2.10.1                                          |
| `@tauri-apps/cli`                       | —          | 2.12.1 (`package-lock.json`)                                     |
| `libloading`                            | —          | 0.8.9 (diretta), 0.7.4 (transitiva)                              |

Risoluzione delle feature per `x86_64-pc-windows-msvc` (`cargo tree -e features`): **[V]**

- `ort-sys` usa solo `load-dynamic`. La feature `download-binaries`, che scarica
  ONNX Runtime nel build script, **non è attiva**.
- Senza `--features asr-vulkan`, `transcribe-cpp-sys` non ha feature di backend:
  la build Windows predefinita è **solo CPU**.
- Con `asr-vulkan` si attiva `transcribe-cpp-sys/vulkan`, quindi `TRANSCRIBE_VULKAN=ON`.

## 3. Comportamento macOS da preservare

Questi sono i punti richiesti dal piano. Riferimenti a `src-tauri/src/`.

### 3.1 Ordine dei tentativi GPU **[V]**

- `engine/providers.rs:97-103`, `gpu_order`:
  - macOS `aarch64` → `[Metal]`;
  - Windows/Linux → `[Cuda, Vulkan]`;
  - qualunque altra combinazione (es. macOS x86_64) → nessuna GPU.
- `providers.rs:117-140`: il registro dei dispositivi è enumerato **una sola volta
  per processo** (`OnceLock`). Restano solo i dispositivi GPU dei backend presenti
  in `gpu_order` più il dispositivo CPU.
- `providers.rs:148-187`, `candidate_indices`:
  - **Auto**: tutte le GPU nell'ordine dei backend, poi una CPU;
  - **dispositivo esplicito**: solo quella GPU, poi la CPU;
  - **CPU esplicita**: solo la CPU.

### 3.2 Condizioni che attivano il fallback **[V]**

`providers.rs:256-296`, `try_candidates`. Un tentativo GPU fallito passa al
candidato successivo solo per questi tre errori:

- `GpuUnavailable`, che comprende `transcribe_cpp::Error::Backend` e il
  dispositivo o backend effettivo diverso da quello richiesto
  (`verify_placement`, `providers.rs:244-253`);
- `ModelLoad`;
- `OutOfMemory`.

Ogni altro errore (`Resources`, `ModelContract`, `NativeCompatibility`,
`Configuration`, `Cancelled`…) **interrompe** il caricamento senza tentare la CPU.

Integrità e contratto dei modelli sono controllati **prima** dei tentativi:
checksum in `engine/transcriber.rs:58`, contratto del modello per ogni candidato
in `transcriber.rs:63-68`. Un modello corrotto o mancante quindi non viene
mascherato dal fallback.

`FallbackReason`:

- `NoGpu` se non è stato tentato nessun GPU;
- `GpuInitializationFailed` se almeno un GPU è fallito;
- `GpuOutOfMemory`, che prevale sugli altri motivi.

Il fallback non si applica agli errori durante l'inferenza
(`docs/architecture.md`: "Inference failures do not automatically repeat
transcription").

### 3.3 Rilascio delle risorse dopo un tentativo fallito **[V]**

Nel closure di caricamento (`transcriber.rs:60-85`) `Model` e `Session` sono
variabili locali: in caso di errore l'operatore `?` le distrugge prima che
`try_candidates` passi al candidato successivo. Il test
`providers/tests.rs::attempts_release_failures_before_cpu` lo verifica con un
contatore di `Drop`.

### 3.4 Selezione e inizializzazione CPU **[V]**

Il candidato CPU è il primo dispositivo registrato con
`classify("cpu", DeviceType::Cpu)` (`providers.rs:106-114`). Per una preferenza
diversa da "CPU", il tentativo CPU viene registrato con
`eprintln!("ASR recovery: backend=CPU reason=…")`.

Esiti possibili:

- fallimento della CPU di recupero → `EngineError::CpuFallbackFailed`;
- nessun dispositivo CPU registrato → `CpuFallbackUnavailable`.

### 3.5 Backend comunicato alla UI **[V]**

- `engine/state.rs:194-210`, `publish_ready`, emette l'evento `transcriber-state`
  con `provider`, `device` (backend e nome verificati da `providers::verify`) e
  `fallbackReason`.
- Lato frontend (`src/components/AsrDeviceControl.tsx:57-66`):
  - mostra sempre "Active ASR (transcribe.cpp): …";
  - se il backend è CPU con un motivo, mostra anche un avviso non bloccante
    (`src/lib/transcription.ts:14-21`) e il pulsante "Reload selection";
  - la VAD è dichiarata come "CPU (ONNX)".

### 3.6 Preferenze esplicite e persistenza **[V]**

- La preferenza del dispositivo (`AsrSelection`) **vale solo per la sessione** e
  torna su Auto a ogni avvio (`providers.rs:51-56`; README "the requested ASR
  device resets to Auto on restart").
- Il **modello ASR** selezionato e la lingua sono invece salvati in `settings.json`
  (`settings.rs`, schema v2).
- Prima di toccare il modello residente, `select` (`state.rs:220-246`) rifiuta gli
  ID dispositivo falsificati o scaduti.

### 3.7 Ricaricamento dopo inattività **[V]**

- Un monitor esegue un controllo ogni secondo (`state.rs:472-484`) e rilascia ASR
  e VAD insieme **dopo più di 300 s** di inattività senza operazioni riservate
  (`state.rs:17`, `73-82`).
- Il job successivo ricarica tutto offline con `get_or_load` → `load`
  (`state.rs:391-413`). La procedura **ricalcola i candidati con la preferenza di
  sessione corrente**, quindi riprova la GPU e ricalcola `fallbackReason`.
- Il registro dei dispositivi non viene ricostruito (`OnceLock`).
- Test di riferimento: `state/tests.rs::idle_deadline_and_reservation` e
  `offline_loader_reuses_and_reloads_once`.

### 3.8 Test che coprono questo comportamento

`engine/providers/tests.rs` (7 test), `engine/state/tests.rs` (9),
`engine/transcriber/tests.rs` (9, di cui 1 ignorato perché richiede pesi reali) e
`src/lib/transcription.test.ts`. Usano sostituti controllati e non richiedono una
GPU. Il piano chiede di riusarli e non duplicarli.

## 4. Codice esclusivo macOS e condizioni di compilazione

| Posizione                                                      | Contenuto                                                                                           |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `src-tauri/Cargo.toml:51-52`                                   | `transcribe-cpp` con la feature `metal` solo per `target_os = "macos"`                              |
| `src-tauri/build.rs:4-21`                                      | Verifica dei checksum di `resources/onnxruntime` e collegamento di `clang_rt.osx` **solo su macOS** |
| `vad/runtime.rs:40-57`                                         | Risoluzione della `.dylib` inclusa nell'app con checksum fisso, solo `cfg(target_os = "macos")`     |
| `vad/runtime.rs:58-62`                                         | Su ogni altro OS, `initialize()` → richiede `ORT_DYLIB_PATH` assoluto                               |
| `engine/mod.rs:3-7`                                            | `compile_error!` se `asr-cuda`/`asr-vulkan` vengono attivate fuori da Windows/Linux                 |
| `providers.rs:97-103`                                          | Ordine GPU per OS e architettura (`std::env::consts`, valutato a runtime)                           |
| `tauri.conf.json:29-31`                                        | `bundle.macOS.minimumSystemVersion`                                                                 |
| `scripts/verify_macos_bundle.py`                               | Verificatore del bundle macOS (otool/lipo/hdiutil)                                                  |
| `engine/model_files/tests.rs:46`, `model_storage/tests.rs:114` | Test `cfg(unix)` (symlink): su Windows non vengono compilati, quindi quella copertura manca         |

`src-tauri/src/main.rs:1` contiene già `windows_subsystem = "windows"` per le
build release. **[V]**

## 5. Dipendenze native Windows e percorsi hardcoded

### 5.1 Percorsi e nomi hardcoded **[V]**

- `vad/runtime.rs:46`: `"onnxruntime/libonnxruntime.1.20.1.dylib"` e il relativo
  SHA-256 (`:53`) sono valori fissi validi solo per macOS.
- `build.rs:6`: `resources/onnxruntime` presume un layout piatto con un solo set di
  file per piattaforma.
- `tauri.conf.json:39-42`: `"resources/onnxruntime/": "onnxruntime/"` copia
  l'intera cartella **su ogni piattaforma**. Una build Windows impacchetterebbe la
  `.dylib` macOS da 25 MB. Questo contrasta con "Includere soltanto runtime della
  piattaforma corretta".
- `resources/onnxruntime/manifest.json` ha un'unica `architecture: "arm64"` e
  nessuna chiave per piattaforma.
- Nessun percorso utente assoluto (es. `/Users/...`) compare nel codice di
  produzione. Le variabili d'ambiente `AURORA_*` sono usate solo dai test ignorati.

### 5.2 ONNX Runtime 1.20.1 win-x64 (artefatto ufficiale) **[V]**

Contenuto di `lib/` rilevante a runtime (le `.lib`/`.pdb` non servono a runtime):

| File                               | SHA-256                                                            | Import PE                                                                                                                                 |
| ---------------------------------- | ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `onnxruntime.dll` (x64, 11,5 MB)   | `4cb41e89b8bf30578e1dd95e9c40292d61974a4bfcd666409302c4f0c5aa8ce0` | `KERNEL32`, `ADVAPI32`, **`MSVCP140`, `MSVCP140_1`, `VCRUNTIME140`, `VCRUNTIME140_1`**, `api-ms-win-crt-*`, `api-ms-win-core-path-l1-1-0` |
| `onnxruntime_providers_shared.dll` | `3b164f0019863266971843baca9551f015db7e502e2d9c4575f74c6a2bbbb78a` | `KERNEL32`, `VCRUNTIME140`, `api-ms-win-crt-runtime`                                                                                      |

Osservazioni:

- `onnxruntime.dll` esporta `OrtGetApiBase`, il simbolo usato dal preflight
  (`vad/runtime.rs:101`).
- `onnxruntime_providers_shared.dll` **non** compare tra gli import di
  `onnxruntime.dll`: viene caricato solo per gli execution provider condivisi.
  Per Silero sulla sola CPU **potrebbe** non servire. **[W]** Va confermato nel
  Milestone 4 prima di escluderlo.
- Entrambe le DLL hanno una directory di sicurezza PE non vuota (firma
  Authenticode presente). **La validità della firma non è stata verificata** in
  questo ambiente.
- `LICENSE`, `ThirdPartyNotices.txt` e `VERSION_NUMBER` del pacchetto Windows
  hanno SHA-256 **diversi** dalle copie macOS nella repository. Il Milestone 2
  dovrà fissare checksum per piattaforma, senza riusare quelli macOS.

### 5.3 Configurazione `ort` e caricamento **[V]**

- `ort` è configurato con `load-dynamic` e senza `download-binaries`.
- Flusso attuale (`vad/runtime.rs`):
  1. `canonicalize`;
  2. preflight con `libloading::Library::new`, che verifica `OrtGetApiBase`, la
     versione `"1.20.1"` e `GetApi(ORT_API_VERSION)`;
  3. `ort::init_from(path).commit()`;
  4. la libreria di preflight resta caricata per tutta la vita del processo; un
     secondo percorso diverso viene rifiutato.
- Su Windows `std::fs::canonicalize` restituisce percorsi verbatim (`\\?\C:\…`). Il
  comportamento di `LoadLibraryExW` usato da `libloading`, e di `ort` con
  questo formato, inclusa la risoluzione delle DLL dipendenti, **non è stato
  verificato**. **[W]**
- Le DLL dipendenti (`MSVCP140*.dll`, `VCRUNTIME140*.dll`) sono risolte
  dall'ordine di ricerca standard di Windows: cartella dell'eseguibile, System32,
  PATH… Questo lega il Milestone 4 alla strategia Visual C++ (§7).

## 6. transcribe.cpp 0.2.4 statico su Windows x64 e Vulkan

### 6.1 Build **[V]**

Fonte: `transcribe-cpp-sys-0.2.4/bindings/rust/sys/build.rs`.

- Il build script esegue CMake sui sorgenti inclusi nel crate. Senza `shared` o
  `dynamic-backends` il risultato è **statico**
  (`TRANSCRIBE_BUILD_SHARED=OFF`), compatibile con il vincolo "Conservare il
  collegamento statico".
- Con MSVC il build script forza `/O2 /Ob2 /DNDEBUG`.
- Il build script **non scarica nulla**. Su Windows crea una junction breve in
  `%LOCALAPPDATA%\tcs\<hash>` per evitare il limite MAX_PATH della build Vulkan;
  se non ci riesce, emette un warning e compila in `OUT_DIR`.
- `TRANSCRIBE_CMAKE_ARGS` / `CMAKE_ARGS` vengono passati a CMake come argomenti
  aggiuntivi.
- Requisiti della build Vulkan (`ggml/src/ggml-vulkan/CMakeLists.txt:9-14`):
  - `find_package(Vulkan COMPONENTS glslc REQUIRED)`;
  - `find_package(SPIRV-Headers CONFIG REQUIRED)`.
- Handy installa SPIRV-Headers con `vcpkg install spirv-headers:x64-windows`,
  perché nel loro CI il Vulkan SDK installato non espone
  `SPIRV-HeadersConfig.cmake` (`build.yml`, step "Install SPIRV-Headers").

### 6.2 Ottimizzazione CPU dipendente dalla macchina di build — **rischio** **[V]**

- `ggml/CMakeLists.txt:105-123` attiva `GGML_NATIVE` di default, a meno di
  cross-compilazione o di `SOURCE_DATE_EPOCH`.
- Con MSVC e `GGML_NATIVE`, `FindSIMD.cmake` rileva le istruzioni (AVX2/AVX512…)
  **della macchina di build** (`ggml-cpu/CMakeLists.txt:249-252`).
- Una build statica prodotta su un runner CI può quindi usare istruzioni assenti
  sul PC dell'utente, con crash per istruzione illegale.
- Handy documenta lo stesso problema per la build statica e imposta
  `TRANSCRIBE_CMAKE_ARGS=-DGGML_NATIVE=OFF -DGGML_OPENMP=OFF`
  (`build.yml:283-300`).
- transcribe.cpp offre `TRANSCRIBE_X86_CONSERVATIVE` (`CMakeLists.txt:159-177`).
  Il livello di istruzioni minimo da adottare è una **decisione aperta** (§12, D3).

### 6.3 Import obbligatorio di `vulkan-1.dll` — **rischio principale di avvio**

- Con Vulkan attivo, il manifest di collegamento aggiunge `vulkan-1` alle librerie
  di sistema su Windows (`cmake/transcribe-install.cmake:138-147`). Il build script
  lo emette come `cargo:rustc-link-lib=dylib=vulkan-1`, cioè come import library di
  `vulkan-1.dll`. **[V]**
- `ggml-vulkan.cpp:4994` chiama direttamente `vkGetInstanceProcAddr`, quindi il
  simbolo è risolto al collegamento. **[V]**
- Nella distribuzione upstream CPU+Vulkan 0.2.4, `ggml-vulkan.dll` importa
  `vulkan-1.dll` **senza delay-load**. Lì il problema non si pone, perché quella
  distribuzione usa il layout a backend dinamici e `ggml-vulkan.dll` è un modulo
  caricato a runtime. **[V]**
- Conclusione dedotta: in una build **statica** con Vulkan, `aurora.exe` avrà un
  import obbligatorio di `vulkan-1.dll`. Su un PC senza loader Vulkan il processo
  **non partirebbe** (errore del loader di Windows prima di `main`) e il fallback
  Rust non verrebbe mai raggiunto. **[W]** Va confermato con
  `dumpbin /DEPENDENTS` nel Milestone 5.
- Dove avviene l'inizializzazione **[V]**:
  - il registro dei backend ggml è inizializzato in modo pigro (`static` locale in
    `get_reg()`, `ggml-backend-reg.cpp:292`), quindi alla prima chiamata a
    `transcribe_cpp::devices()`, cioè nel worker di avvio (`state.rs:445-471`);
  - `ggml_backend_vk_reg()` (`ggml-vulkan.cpp:15885-15904`) intercetta le eccezioni
    C++ di `ggml_vk_instance_init()` e restituisce `nullptr`. Se il **loader è
    presente** ma manca un driver o Vulkan 1.2, il backend non si registra e resta
    solo la CPU, senza crash.
- Con `/DELAYLOAD:vulkan-1.dll`, l'assenza della DLL genererebbe un'eccezione SEH
  (`0xC06D007E`) alla prima chiamata. Il comportamento di `catch (...)` con le
  eccezioni SEH dipende dal modello `/EH` usato per compilare ggml, che non è stato
  verificato. **[W]**

Opzioni da valutare nel Milestone 5 (§12, D1). Nessuna è stata implementata.

### 6.4 Distribuzione upstream CPU+Vulkan **[V]**

La release `v0.2.4` contiene `transcribe-native-0.2.4-windows-x86_64-cpu-vulkan.tar.gz`
(`contract.json`: `lane: "cpu-vulkan"`, `header_hash: 7df72bf9e667b8c2`). Contenuto:

- `transcribe.dll`, `ggml.dll`, `ggml-base.dll`;
- `ggml-vulkan.dll`;
- 9 varianti `ggml-cpu-*.dll` (da `sse42` ad `alderlake`);
- licenze.

È il layout **condiviso con backend dinamici**, non statico. Come prevede il piano,
vale solo come riferimento: la sua esistenza non dimostra che la build statica di
AURORA funzioni. Tutte le sue DLL importano `MSVCP140`, `VCRUNTIME140` e
`VCRUNTIME140_1`.

## 7. Runtime Visual C++

- **ONNX Runtime** richiede `MSVCP140.dll`, `MSVCP140_1.dll`, `VCRUNTIME140.dll` e
  `VCRUNTIME140_1.dll` (§5.2). Il runtime Visual C++ serve quindi **comunque**,
  qualunque scelta si faccia per l'eseguibile. **[V]**
- **Tauri** ha `build.windows.staticVCRuntime` **attivo di default**
  (`tauri-utils-2.10.1/src/config.rs:3905-3926`). `tauri-build` sostituisce
  `msvcrt.lib` con una libreria vuota e forza `libcmt.lib` e `libvcruntime.lib`
  (`tauri-build-2.7.1/src/static_vcruntime.rs`). **[V]**
- **transcribe.cpp** è compilato da CMake con MSVC. Il build script non imposta
  `CMAKE_MSVC_RUNTIME_LIBRARY`, quindi gli archivi statici usano il default di
  CMake, cioè il runtime dinamico `/MD`. **[W]**
  - Collegare archivi `/MD` con il CRT statico forzato da Tauri può produrre
    errori di collegamento (`LNK2038` RuntimeLibrary mismatch, simboli `__imp_*`
    irrisolti) o un import residuo di `MSVCP140.dll`. **Va verificato con la prima
    build Windows**.
  - Handy dichiara esplicitamente che il suo stack nativo usa `/MD` e copia le DLL
    del runtime accanto all'eseguibile (`build.rs`, `stage_vc_runtime_dlls`).
- `tauri-utils` 2.10.1 espone `bundle.windows.bundleVCRuntime`: copia le DLL del
  runtime accanto all'applicazione (`config.rs:1084-1095`). Il supporto effettivo
  nel CLI 2.12.1 e l'elenco dei file copiati **non sono stati verificati**. **[W]**
- La strategia (DLL accanto all'eseguibile oppure redistribuibile Microsoft
  installato da NSIS) è una **decisione aperta** (§12, D2), da prendere nel
  Milestone 3 dopo la prima build.

## 8. Packaging Tauri

| Aspetto          | Stato attuale                        | Osservazione                                                                                                                                             |
| ---------------- | ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bundle.targets` | `"all"`                              | Su Windows produrrebbe MSI e NSIS. Il piano chiede NSIS: va limitato con `tauri.windows.conf.json` o con il flag `--bundles nsis`                        |
| WebView2         | non configurato                      | Il default di Tauri è `downloadBootstrapper { silent: true }` (`config.rs:1001-1005`), cioè già la scelta del piano. Conviene dichiararlo esplicitamente |
| Risorse          | `resources/onnxruntime/` per ogni OS | Va separato per piattaforma (§5.1)                                                                                                                       |
| Firma            | assente                              | Fuori scopo (Milestone 8, attività separata)                                                                                                             |
| CI               | `.github/` assente                   | Nessun workflow esistente                                                                                                                                |

Il CLI di Tauri potrebbe scaricare il toolset NSIS durante il bundling, se non è
già in cache. Non l'ho verificato. Andrà controllato nel Milestone 3 rispetto al
vincolo "nessun download nascosto". **[W]**

## 9. Confronto con Handy

| Area                    | Handy (`5a9451e`)                                                                                 | AURORA                      | Applicabile?                                                                                          |
| ----------------------- | ------------------------------------------------------------------------------------------------- | --------------------------- | ----------------------------------------------------------------------------------------------------- |
| Versione transcribe-cpp | **0.4.0**                                                                                         | **0.2.4**                   | Differenza di versione: le configurazioni non si possono copiare così come sono                       |
| Layout Windows x64      | `dynamic-backends` + `vulkan`: DLL condivise e moduli ggml caricati a runtime                     | statico (vincolo del piano) | **No**: è proprio il meccanismo che rende Vulkan opzionale in Handy, ma viola il collegamento statico |
| Windows ARM64           | statico CPU, `-DGGML_NATIVE=OFF`                                                                  | fuori scopo                 | L'impostazione `GGML_NATIVE=OFF` per le build statiche **sì**                                         |
| ONNX Runtime            | **non più usato** (commento in `build.rs`)                                                        | Silero via ORT 1.20.1       | Nessun riferimento attuale in Handy per ORT su Windows                                                |
| Runtime VC++            | DLL `msvcp140*`/`vcruntime140*` copiate da Visual Studio accanto all'exe (`HANDY_VC_REDIST_DIRS`) | non gestito                 | Approccio candidato per D2                                                                            |
| Vulkan SDK in CI        | `humbletim/install-vulkan-sdk@v1.2`, versione `1.4.309.0`                                         | —                           | Riferimento per il Milestone 6. Versione e action vanno fissate e verificate                          |
| SPIRV-Headers           | `vcpkg install spirv-headers:x64-windows`                                                         | —                           | Probabilmente sì (stesso `find_package` in 0.2.4) **[W]**                                             |
| NSIS                    | template personalizzato (modalità portable) basato su Tauri 2.9.1                                 | template predefinito        | Non necessario per il piano                                                                           |
| CI                      | `tauri-apps/tauri-action@v0`, `actions/checkout@v5`, `swatinem/rust-cache@v2`, …                  | —                           | Riferimento; il piano vieta release automatiche, quindi niente upload su release                      |

## 10. Requisiti

### Macchina di build (Windows x64)

Dedotto dai sorgenti; da confermare nel Milestone 5/6. **[W]**

- Visual Studio 2022 (o Build Tools) con il carico di lavoro C++ e Windows SDK;
  target Rust `x86_64-pc-windows-msvc`.
- CMake ≥ 3.19 (minimo di `ggml-vulkan`) e Node.js con `--experimental-strip-types`
  (comando `npm test`).
- Per `asr-vulkan`: Vulkan SDK con `glslc`, `vulkan-1.lib`, header Vulkan e
  SPIRV-Headers trovabili da CMake.
- Risorse ONNX Runtime win-x64 preparate dallo script del Milestone 2.

### PC dell'utente

- Windows x64 con WebView2 (scaricato dal bootstrapper se assente; serve Internet
  durante l'installazione).
- Runtime Visual C++ 2015-2022 x64, secondo la strategia D2.
- Per l'accelerazione: driver GPU con loader Vulkan (`vulkan-1.dll`, installato dai
  driver) e Vulkan ≥ 1.2. **Senza driver l'app deve comunque avviarsi** (§6.3).

## 11. Rischi di avvio e incompatibilità

| #   | Rischio                                                                                                                      | Evidenza     | Milestone |
| --- | ---------------------------------------------------------------------------------------------------------------------------- | ------------ | --------- |
| R1  | Import obbligatorio di `vulkan-1.dll` in `aurora.exe` statico con `asr-vulkan`                                               | §6.3 [V]+[W] | M5        |
| R2  | Istruzioni CPU della macchina di build (`GGML_NATIVE=ON`) → crash su CPU più vecchie                                         | §6.2 [V]     | M5/M6     |
| R3  | Conflitto CRT statico di Tauri ↔ archivi `/MD` di transcribe.cpp                                                             | §7 [W]       | M3        |
| R4  | Runtime VC++ assente → `onnxruntime.dll` non si carica (VAD non disponibile)                                                 | §5.2 [V]     | M3/M4     |
| R5  | Su Windows la VAD richiede `ORT_DYLIB_PATH`, quindi un'installazione normale non trascrive                                   | §4 [V]       | M4        |
| R6  | `.dylib` macOS impacchettata su Windows; nessuna verifica delle risorse non-macOS in `build.rs`                              | §5.1 [V]     | M2/M3     |
| R7  | Percorsi verbatim `\\?\` da `canonicalize` passati a `LoadLibraryExW`/`ort`                                                  | §5.3 [W]     | M4        |
| R8  | MSI generato oltre a NSIS (`targets: "all"`)                                                                                 | §8 [V]       | M3        |
| R9  | Rifiuto dei symlink in `model_files.rs` (`is_symlink`) con junction NTFS non verificato; test `cfg(unix)` assenti su Windows | §4 [W]       | M5/M7     |

### Incompatibilità con le istruzioni del progetto (segnalate, non risolte)

1. **CUDA**:
   - `providers.rs:100` mette CUDA prima di Vulkan su Windows, ed esistono la
     feature `asr-cuda` e i testi UI "CUDA then Vulkan";
   - il piano dichiara CUDA fuori scopo;
   - senza la feature, CUDA non compare tra i dispositivi registrati, quindi non
     serve cambiare codice per escluderla. Va però deciso se aggiornare i testi.
2. **AGENTS.md §4.3 e checklist §5** parlano di execution provider
   CoreML/DirectML per ONNX. Il codice e il piano mantengono Silero **solo su
   CPU**, quindi DirectML non va introdotto.
3. **AGENTS.md §4.2** chiede di inizializzare il motore una volta all'avvio. Il
   codice rilascia i modelli dopo 300 s e li ricarica; il piano chiede di
   preservare questo comportamento.
4. Il piano vieta di aggiungere "una funzione di cancellazione dei modelli": esiste
   già (`delete_model`) e resta invariata.
5. `AGENTS.md` e `message.md` sono git-ignorati: le istruzioni non sono versionate
   nel fork.

## 12. Decisioni aperte

Da prendere prima della milestone indicata. Le opzioni sono proposte, nessuna è stata applicata.

**D1 — Avvio senza loader Vulkan con transcribe.cpp statico (M5)**

| Opzione                                                                                                                      | Pro                                                    | Contro                                                                                                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| a) `/DELAYLOAD:vulkan-1.dll` + controllo in Rust (`LoadLibraryExW` da System32) **prima** del primo accesso al registro ggml | Mantiene il collegamento statico e un solo eseguibile  | Il registro ggml registra comunque Vulkan: se il loader manca serve un modo per non arrivare a `vkGetInstanceProcAddr` (gestione SEH o hook di delay-load); richiede `unsafe` e prove su Windows |
| b) Hook di fallimento delay-load che restituisce uno stub (`vkGetInstanceProcAddr` → `NULL`)                                 | Nessuna modifica a ggml                                | Comportamento di `vk::enumerateInstanceVersion` con dispatcher nullo da verificare; codice nativo da mantenere                                                                                   |
| c) Passare a `shared` + `dynamic-backends` (come Handy)                                                                      | Vulkan opzionale per costruzione, già provato upstream | **Viola** "Conservare il collegamento statico". Richiede l'approvazione esplicita di un cambio di architettura                                                                                   |

**D2 — Runtime Visual C++ (M3)**

| Opzione                                                                                                  | Pro                                                    | Contro                                                                                                                 |
| -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| a) DLL del runtime accanto all'eseguibile (app-local, come Handy, o con `bundleVCRuntime` se confermato) | Nessun download o privilegio in installazione; offline | Va rispettata la licenza di redistribuzione e incluso l'avviso; aggiornamenti di sicurezza a carico dell'app           |
| b) Redistribuibile ufficiale `vc_redist.x64.exe` eseguito dall'installer NSIS                            | Aggiornabile da Windows Update; condiviso              | Richiede admin, gestione dei codici di uscita e del riavvio, download o inclusione del pacchetto, verifica della firma |

In entrambi i casi va deciso anche `staticVCRuntime` (R3).

**D3 — Livello di istruzioni CPU minimo per transcribe.cpp (M5)**

| Opzione                                                                                | Pro                                          | Contro                                       |
| -------------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| a) `-DGGML_NATIVE=OFF` + `-DGGML_AVX2=ON` (Intel Haswell / AMD Excavator e successivi) | Prestazioni buone sulla maggior parte dei PC | Non parte su CPU senza AVX2                  |
| b) `TRANSCRIBE_X86_CONSERVATIVE=ON` (x86-64 di base)                                   | Massima compatibilità                        | Inferenza CPU sensibilmente più lenta        |
| c) Lasciare `GGML_NATIVE` (default)                                                    | Nessuna configurazione                       | Il binario dipende dalla CPU del runner (R2) |

**D4 — Formato del report e dei documenti futuri**

Il resto della documentazione della repository è in inglese; questo report è in
italiano come `message.md`. Va deciso se tradurlo.

## 13. Componenti da adattare (mappa per milestone)

| Milestone | File / componente                                                                                                                                                                   |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M2        | `resources/onnxruntime/`: layout per piattaforma (es. `macos-arm64/`, `windows-x64/`), manifest per piattaforma, script di preparazione con checksum fissati (archivio `78d44705…`) |
| M3        | `tauri.windows.conf.json` (NSIS, WebView2, risorse Windows), `build.rs` (verifica delle risorse per target, errore per target non supportati), strategia VC++                       |
| M4        | `vad/runtime.rs`: ramo `cfg(windows)` che risolve `onnxruntime.dll` dal layout installato, checksum Windows, ricerca sicura delle dipendenze                                        |
| M5        | Feature `asr-vulkan` nella build Windows, `TRANSCRIBE_CMAKE_ARGS`, D1/D3, test del fallback riusando `try_candidates`                                                               |
| M6        | `.github/workflows/windows-ci.yml` (nuovo)                                                                                                                                          |
| M8        | `README.md`, `docs/development.md`, workflow di release manuale                                                                                                                     |

Il codice di fallback (`providers.rs`, `transcriber.rs`, `state.rs`) e la UI sono
**già indipendenti dalla piattaforma**. Per Windows serve abilitarli con Vulkan,
non duplicarli.

## 14. Verifiche non eseguite in questo milestone

- Nessuna build Windows, `dumpbin`, avvio senza Vulkan o inferenza GPU: l'ambiente
  non le permette.
- Firma Authenticode delle DLL ONNX Runtime non validata.
- Comportamento di `bundleVCRuntime` e del download di NSIS nel CLI Tauri 2.12.1.
- Test esistenti (`cargo test`, `npm test`) non eseguiti: il milestone non modifica
  codice. Sono richiesti prima dei Milestone che lo modificano.
- Audio di test (`audio_samples/` da 10/30/60 minuti) non disponibile nella
  sessione. Non serve per questo milestone.
