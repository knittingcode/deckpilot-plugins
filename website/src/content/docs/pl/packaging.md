---
title: "Pakowanie"
description: "Budowanie modułu dla wasm32-unknown-unknown, tworzenie pakietu .dpplugin narzędziem deckpilot-registry, limity rozmiaru, powtarzalne buildy, build.json."
group: ship
order: 13
---

Wtyczka jest dystrybuowana jako jeden plik, `<id>-<version>.dpplugin`. Zawiera on manifest, skompilowany moduł, licencję i zasoby wymienione w manifeście. Ta strona omawia budowanie modułu, tworzenie i sprawdzanie pakietu, limity rozmiaru, to, co wiadomo o powtarzalnych buildach, oraz przepis budowania, którego rejestr używa do ponownego zbudowania twojej wtyczki.

> **Uwaga:** Pakowanie to nie publikowanie. Pakiet, który budujesz samodzielnie, służy do sprawdzania i testów lokalnych. Pakiety trafiają do użytkowników wyłącznie przez rejestr, który buduje je ponownie z twojego otagowanego kodu źródłowego; zobacz [Zgłaszanie wtyczki](../submission/).

## Układ projektu

```text
your-plugin/
├── Cargo.toml          # [lib] crate-type = ["cdylib", "rlib"]
├── Cargo.lock          # committed
├── rust-toolchain.toml # an exact toolchain version
├── manifest.json
├── LICENSE             # required in every package
├── README.md           # packaged only if the manifest names it
├── assets/icon-128.png # icons.128: a 128 by 128 pixel PNG, at most 256 KiB
└── src/lib.rs          # plugin() and deckpilot_plugin_sdk::plugin!(plugin)
```

Zależności muszą się budować dla `wasm32-unknown-unknown` bez WASI i bez kodu pośredniczącego JavaScript: bez `wasm-bindgen` i bez generatora liczb losowych, który oczekuje przeglądarki. Kontrola pakietu odrzuca moduł z jakimkolwiek importem innym niż `deckpilot_v1.host_call`.

## Budowanie modułu

We własnym repozytorium, z profilem `guest` opisanym na stronie [Pierwsze kroki](../getting-started/):

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked --target wasm32-unknown-unknown --profile guest
```

Moduł jest zapisywany w `target/wasm32-unknown-unknown/guest/<crate_name>.wasm`.

Profil `guest` dziedziczy po `release` i ustawia `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `panic = "abort"` oraz `strip = true`. Z tym profilem przykład Hello World ma około 190 KB, a Developer Tools około 300 KB.

Dla przykładów w kopii roboczej repozytorium DeckPilota istnieje skrypt, który dodatkowo przemapowuje ścieżki budowania i wypisuje sha256 każdego modułu:

```sh
plugins/examples/build.sh                  # hello-world and developer-tools
plugins/examples/build.sh developer-tools  # one
```

Przyjmuje opcje `--target-dir DIR` oraz `--out DIR` i domyślnie zapisuje wynik w `plugins/runtime/target/examples/<crate>.wasm`.

## Format pakietu

Plik `.dpplugin` to archiwum tar skompresowane gzipem, w stałej, deterministycznej postaci: wpisy posortowane według ścieżki, stałe uprawnienia plików i znaczniki czasu, bez wpisów katalogów. Zawiera dokładnie:

| Plik | Wymagany |
| --- | --- |
| `manifest.json` | tak |
| `plugin.wasm` | tak |
| `LICENSE` | tak |
| Ikony, ikony przycisków, zrzuty ekranu, `README.md` i `CHANGELOG.md` wymienione w manifeście | zgodnie z manifestem |

Nic innego nie może się znaleźć w pakiecie.

## Polecenie package

Narzędzie rejestru `deckpilot-registry` znajduje się w repozytorium DeckPilota, w crate `deckpilot-plugin-registry`, za funkcją Cargo `publish`. Z kopii roboczej repozytorium DeckPilota:

```sh
cargo run --locked -p deckpilot-plugin-registry --features publish --bin deckpilot-registry -- \
  package --plugin-dir plugins/examples/developer-tools \
  --module plugins/runtime/target/examples/developer_tools.wasm \
  --out developer-tools-0.1.0.dpplugin
```

| Opcja | Znaczenie |
| --- | --- |
| `--plugin-dir DIR` | Katalog z `manifest.json`, `LICENSE` i plikami wymienionymi w manifeście. Może się znajdować poza kopią roboczą repozytorium |
| `--module FILE.wasm` | Zbudowany moduł; jest zapisywany jako `plugin.wasm` |
| `--out FILE.dpplugin` | Pakiet do zapisania |

Po powodzeniu polecenie wypisuje identyfikator wtyczki, wersję, ścieżkę wyjściową, sha256 i rozmiar pakietu.

`package` wykonuje każdą kontrolę, którą wykonałby odbiorca pakietu:

- pełną walidację manifestu, w tym reguły dotyczące wielu pól naraz, których JSON Schema nie potrafi wyrazić;
- limity rozmiaru opisane poniżej;
- statyczną kontrolę modułu: poprawny moduł rdzenia WebAssembly, wyłącznie import `deckpilot_v1.host_call`, wymagane eksporty o dokładnie określonych typach.

Wynik jest deterministyczny: te same dane wejściowe dają te same bajty.

## Limity rozmiaru

| Co | Limit |
| --- | --- |
| Pakiet, skompresowany | 5 MiB |
| Dowolny pojedynczy plik | 5 MiB |
| Wszystkie pliki po rozpakowaniu | 10 MiB |
| `plugin.wasm` | 4 MiB |
| Wpisy w archiwum | 256 |
| `manifest.json` | 256 KiB |
| Ikona 128 pikseli pokazywana przez agenta | 256 KiB |

## Powtarzalne buildy

Rejestr nie przyjmuje twojego pliku binarnego. Buduje moduł ponownie z twojego kodu źródłowego i wymaga, aby jego własny build zgadzał się z zapisanym skrótem, dlatego ma znaczenie, na ile build jest powtarzalny. Oto, co zmierzono dla przykładowych wtyczek z toolchainem 1.99.1:

| Porównanie | Wynik |
| --- | --- |
| Dwa buildy na jednej maszynie, każdy do nowego, pustego katalogu target | Identyczne co do bajta, na Ubuntu, macOS i Windows. CI sprawdza to przy każdej zmianie |
| Dwie maszyny z tym samym systemem operacyjnym | Identyczne dla Hello World, różne dla Developer Tools; nie analizowano tego dalej |
| Różne systemy operacyjne | Różne dla obu modułów |

Wnioski:

- Przypnij toolchain do dokładnej wersji i zacommituj `Cargo.lock`.
- Nie oczekuj, że build na twojej maszynie będzie zgodny z buildem rejestru. Referencyjny build rejestru działa w CI na `ubuntu-24.04`, x86_64, i to jego skrót się liczy.
- Zadbaj o hermetyczność buildu: żadnego skryptu budowania, który cokolwiek pobiera lub wykonuje, i żadnego dostępu do sieci poza zablokowanymi zależnościami Cargo.

## Przepis budowania

W repozytorium rejestru każda wtyczka ma plik `plugins/<id>/build.json`, który mówi rejestrowi, jak zbudować moduł. Dla wtyczki, w której katalog główny repozytorium jest jednocześnie crate'em:

```json
{
  "id": "acme.status-check",
  "toolchain": "1.99.1",
  "workspace": ".",
  "package": "status-check",
  "profile": "guest",
  "artifact": "status_check.wasm"
}
```

| Pole | Znaczenie |
| --- | --- |
| `id` | Identyfikator wtyczki |
| `toolchain` | Dokładna wersja Rusta, na przykład `1.99.1`, instalowana przez rustup |
| `workspace` | Katalog workspace'u Cargo z jego `Cargo.lock`, względem katalogu głównego repozytorium; `.` oznacza katalog główny |
| `package` | Pakiet Cargo, który buduje moduł |
| `profile` | Profil Cargo, na przykład `release` lub `guest` |
| `artifact` | Nazwa pliku modułu w `target/wasm32-unknown-unknown/<profile>/` |

Wszystkie sześć pól jest wymaganych. Przepis nie dopuszcza niczego więcej: żadnych własnych poleceń, żadnych funkcji Cargo (features), żadnych zmiennych środowiskowych. Skrypt budujący rejestru pobiera dokładnie zapisany commit, uruchamia `cargo build --locked` dla pakietu, profilu i toolchainu z przepisu, dla celu `wasm32-unknown-unknown` i z przemapowanymi prefiksami ścieżek, a następnie uruchamia `deckpilot-registry package`.

Schemat to [schemas/inputs/build-recipe.schema.json](https://github.com/knittingcode/deckpilot-plugins/blob/main/schemas/inputs/build-recipe.schema.json), a własny [przykładowy przepis](https://github.com/knittingcode/deckpilot-plugins/blob/main/plugins/examples/hello-weather/build.json) rejestru buduje przykład Hello Weather z workspace'u DeckPilota.

## Lista kontrolna

- `manifest.json` przechodzi walidację i zgadza się z kodem.
- `LICENSE` i ikona 128 pikseli są obecne.
- Moduł buduje się z `--locked` z czystej kopii roboczej repozytorium.
- `deckpilot-registry package` kończy się powodzeniem.
- Pakiet nie jest nigdzie commitowany. Repozytorium rejestru odrzuca pliki `.dpplugin`, a pakiet nigdy nie jest dołączany do wydania aplikacji DeckPilot.

## Dokumentacja źródłowa

[docs/marketplace/sdk/building.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/building.md), [docs/marketplace/artifact-contract.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/artifact-contract.md) oraz [README](https://github.com/knittingcode/deckpilot/blob/main/crates/deckpilot-plugin-package/README.md) crate'a pakietu.
