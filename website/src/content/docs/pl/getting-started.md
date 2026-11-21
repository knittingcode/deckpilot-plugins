---
title: "Pierwsze kroki"
description: "Przygotuj toolchain, pobierz SDK wtyczek dla Rusta z repozytorium DeckPilota, a potem zbuduj i przetestuj najmniejszą przykładową wtyczkę."
group: start
order: 1
---

Wtyczka DeckPilota to crate Rusta skompilowany do jednego modułu WebAssembly, dostarczany razem z plikiem `manifest.json`, który ją opisuje. Ta strona prowadzi od pustej maszyny do zbudowanego i przetestowanego przykładu oraz pokazuje, jak twój własny crate korzysta z SDK jako zależności.

> **Status:** DeckPilot [0.10.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.10.0), wydany 2026-11-21, jest bieżącym wydaniem i udostępnia API wtyczek 1.9. Wydany 2026-11-18 [0.7.0](https://github.com/knittingcode/deckpilot/releases/tag/v0.7.0) jest pierwszym wydaniem z obsługą wtyczek. Każde kolejne wydanie zawiera host wtyczek WebAssembly w piaskownicy w każdym instalatorze i wbudowane klucze główne rejestru. Od wersji 0.8.0 Marketplace otwiera się z paska narzędzi głównego okna; w wersji 0.10.0 zainstalowanymi wtyczkami zarządza się w Settings › Plugins, a w wersji 0.7.0 strona Marketplace znajduje się w Settings › Extensions › Plugins. Wtyczki są domyślnie włączone; wyłącza je flaga `--disable-plugins` lub zmienna `DECKPILOT_ENABLE_PLUGINS=0`. Git, Docker, Workspace, Projects, GitHub, GitLab, Jira i Workflows oraz powiadomienia DeckPilota działają jako własne wtyczki DeckPilota dołączone do aplikacji. Podpisany rejestr działa pod adresem `https://knittingcode.github.io/deckpilot-plugins/v1/` i publikuje własne wtyczki DeckPilota; nie jest jeszcze otwarty dla innych wydawców. Crate SDK nie jest opublikowany w crates.io. Ta dokumentacja opisuje API wtyczek 1.x, ABI gościa 1 i wersję manifestu 1.

## Wymagania wstępne

| Narzędzie | Po co |
| --- | --- |
| `git` | SDK i przykłady znajdują się w repozytorium DeckPilota |
| `rustup` | Repozytorium przypina swój toolchain Rusta (1.99.1) w pliku `rust-toolchain.toml`; rustup instaluje go przy pierwszym użyciu |
| Target `wasm32-unknown-unknown` | Wtyczki są budowane właśnie dla niego, bez WASI |

Node.js nie jest potrzebny. Pakiet JavaScript w repozytorium zawiera wyłącznie deklaracje typów i nie pozwala zbudować wtyczki; zobacz [Przegląd SDK](../sdk/#javascript).

## Skąd wziąć SDK

SDK dla Rusta to crate `deckpilot-plugin-sdk` w wersji 0.1.0, w katalogu `plugins/runtime/guest/plugin-sdk` repozytorium [knittingcode/deckpilot](https://github.com/knittingcode/deckpilot). Jest oznaczony jako `publish = false` i nie ma go w crates.io. Można dodać go jako zależność na dwa sposoby.

### W kopii roboczej repozytorium

Przykładowe wtyczki należą do workspace'u Cargo `plugins/runtime` i dziedziczą z niego tę zależność:

```toml
[package]
name = "hello-world"
description = "Example DeckPilot plugin: one pure action, no permissions"
version = "0.1.0"
edition = "2024"
rust-version = "1.90"
license = "Apache-2.0"
publish = false
workspace = "../../runtime"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
deckpilot-plugin-sdk.workspace = true
```

Typ crate'a `cdylib` daje moduł. Typ `rlib` pozwala uruchamiać testy jednostkowe natywnie, zwykłym `cargo test`. Nową wtyczkę umieszczoną obok przykładów trzeba też dodać do list `members` i `default-members` w pliku `plugins/runtime/Cargo.toml`.

### Z własnego repozytorium

Wtyczka zgłaszana do rejestru znajduje się w repozytorium swojego autora, więc nie może użyć ścieżki w workspace'ie. Działa zależność git przypięta do commita:

```toml
[dependencies]
deckpilot-plugin-sdk = { git = "https://github.com/knittingcode/deckpilot", rev = "7f614b521a7b858d26b3f018da946e6a515cbcd4" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.guest]
inherits = "release"
opt-level = "s"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

Cargo znajduje crate wewnątrz repozytorium i rozwiązuje jego pola dziedziczone z workspace'u. Trzy rzeczy nie przenoszą się z workspace'u DeckPilota, więc twoje repozytorium musi mieć własne: zacommitowany plik `Cargo.lock`, plik `rust-toolchain.toml` z dokładną wersją oraz pokazany wyżej profil budowania `guest` (to kopia profilu z `plugins/runtime/Cargo.toml`).

> **Uwaga:** Ta konfiguracja została sprawdzona na potrzeby tych stron na cracie spoza repozytorium: testy natywne, build dla `wasm32-unknown-unknown` z toolchainem 1.99.1 i pakowanie powiodły się względem commita podanego wyżej. Nie przeszła natomiast przez CI rejestru, którego jedyny przykład buduje się wewnątrz workspace'u DeckPilota.

Zawsze przypinaj `rev`. SDK ma wersję 0.1.0 i późniejszy commit może zmienić jego API w Ruście.

## Zbuduj przykład

Sklonuj repozytorium i zbuduj Hello World, najmniejszy przykład: jedna akcja, jeden parametr, żadnych uprawnień.

```sh
git clone https://github.com/knittingcode/deckpilot
cd deckpilot
rustup target add wasm32-unknown-unknown
plugins/examples/build.sh hello-world
```

Skrypt uruchamia `cargo build --locked` w katalogu `plugins/runtime` dla targetu `wasm32-unknown-unknown` z profilem `guest`, kopiuje moduł do `plugins/runtime/target/examples/hello_world.wasm` i wypisuje jego sha256.

Cała wtyczka to ta definicja i jeden handler, z pliku `plugins/examples/hello-world/src/lib.rs`:

```rust
pub fn plugin() -> Plugin {
    Plugin::new("acme.hello-world", "0.1.0")
        .action(Action::new("greet", greet).param(Param::text("name").required().max_length(64)))
}

// The guest ABI 1 exports (`dp_abi_version`, `dp_alloc`, `dp_invoke`, ...).
deckpilot_plugin_sdk::plugin!(plugin);

/// `greet`: "Hello, <name>!". The SDK has already checked that `name` is
/// present, is text and is at most 64 characters.
fn greet(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let name: String = args.get("name")?;
    let name = name.trim();
    if name.is_empty() {
        return Err(PluginError::new(
            codes::INVALID_PARAMETERS,
            "parameter `name` must not be blank",
        ));
    }
    let greeting = format!("Hello, {name}!");
    Ok(Output::message(greeting.clone()).with("greeting", greeting))
}
```

Obok leżą: plik `manifest.json`, który deklaruje ten sam identyfikator, wersję, akcję i parametr, plik `LICENSE` oraz ikona PNG o wymiarach 128 na 128 pikseli. Każde pole wyjaśnia strona [Format manifestu](../manifest/).

## Uruchom testy

Testy jednostkowe wtyczki uruchamiają się natywnie, bez WebAssembly i bez agenta DeckPilota:

```sh
cd plugins/runtime
cargo test --locked -p hello-world
```

Testy przykładu sprawdzają, czy kod i `manifest.json` są ze sobą zgodne, walidują manifest względem JSON Schema i wywołują akcję przez atrapę hosta z SDK. Zobacz [Testowanie](../testing/).

## Utwórz pakiet

Pakiet `.dpplugin` buduje narzędzie rejestru dostarczane w tym samym repozytorium. Z katalogu głównego repozytorium:

```sh
cargo run --locked -p deckpilot-plugin-registry --features publish --bin deckpilot-registry -- \
  package --plugin-dir plugins/examples/hello-world \
  --module plugins/runtime/target/examples/hello_world.wasm \
  --out hello-world-0.1.0.dpplugin
```

Pakowanie to nie publikowanie. Dowodzi jedynie, że manifest, moduł i pliki mieszczą się w formacie; zobacz [Pakowanie](../packaging/).

## Co dalej

| Cel | Strona |
| --- | --- |
| Zrozumieć, jak działa wtyczka i do czego ma dostęp | [Architektura](../architecture/) |
| Poznać typy SDK | [Przegląd SDK](../sdk/) |
| Napisać manifest | [Format manifestu](../manifest/) |
| Zadeklarować akcje i parametry | [Akcje](../actions/), [Parametry i ustawienia](../parameters/) |
| Wywoływać sieć, przechowywać dane, używać sekretów | [Uprawnienia](../permissions/), [Dane uwierzytelniające](../credentials/), [Pamięć trwała](../storage/), [Zmienne](../variables/) |
| Wypróbować wtyczkę w prawdziwym agencie | [Testowanie](../testing/) |
| Przygotować zgłoszenie | [Zgłaszanie wtyczki](../submission/) |

Kanoniczna dokumentacja SDK znajduje się w repozytorium DeckPilota: [docs/marketplace/sdk/README.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/README.md). Pozostałe dwa przykłady to [Developer Tools](https://github.com/knittingcode/deckpilot/blob/main/plugins/examples/developer-tools/README.md), który używa każdej funkcji hosta (capability), oraz [Hello Weather](https://github.com/knittingcode/deckpilot/blob/main/plugins/examples/hello-weather/README.md), który wykonuje jedno żądanie za pośrednictwem brokera uprawnień. To tylko przykłady i nigdy nie są publikowane.
