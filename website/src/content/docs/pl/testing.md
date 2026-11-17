---
title: "Testowanie"
description: "Testy jednostkowe wtyczki uruchamiane natywnie z MockHost i manifest_mismatches, a potem próba z prawdziwym agentem i lokalnym, jednorazowym rejestrem."
group: ship
order: 12
---

Wtyczkę można testować na dwóch poziomach. Testy jednostkowe działają natywnie przez `cargo test`: atrapa hosta z SDK zastępuje agenta, więc nie biorą w nich udziału ani WebAssembly, ani DeckPilot. Do sprawdzenia całej ścieżki lokalnie zbudowany agent instaluje wtyczkę z lokalnego rejestru podpisanego jednorazowymi kluczami.

## Natywne testy jednostkowe

Ponieważ crate ma także typ `rlib`, jego testy są zwykłymi testami Rusta:

```sh
cargo test --locked
```

W kopii roboczej repozytorium DeckPilota uruchom to polecenie w `plugins/runtime` z opcją `-p <your-plugin>`.

Moduł `deckpilot_plugin_sdk::testing` udostępnia dwa narzędzia: `MockHost` i `manifest_mismatches`.

## Zgodność kodu z manifestem

`manifest_mismatches(&plugin, &manifest)` wypisuje każdą różnicę między definicją w kodzie a `manifest.json`: identyfikator, wersję, `engines.pluginApi`, `engines.sdk`, identyfikatory akcji i ich kolejność oraz `required`, rodzaj, zakresy, opcje i wartość domyślną każdego parametru. Sprawdź asercją, że lista jest pusta.

```rust
const MANIFEST: &str = include_str!("../manifest.json");

#[test]
fn definition_is_valid_and_matches_the_manifest() {
    plugin().validate().unwrap();
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(manifest_mismatches(&plugin(), &manifest), Vec::<String>::new());
}
```

Drugi test pilnuje zgodności kodów błędów:

```rust
#[test]
fn every_own_code_is_declared() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    let declared = manifest["contributes"]["errorCodes"].as_object().unwrap();
    for code in PLUGIN_CODES {
        assert!(declared.contains_key(*code), "{code} is not declared");
        assert!(!codes::is_shared(code), "{code} is a shared code");
    }
}
```

### Walidacja względem schematu

`manifest_mismatches` porównuje kod z manifestem; nie waliduje samego manifestu. Przykładowe wtyczki walidują swoje manifesty w teście jednostkowym względem [plugins/schemas/plugin-manifest.schema.json](https://github.com/knittingcode/deckpilot/blob/main/plugins/schemas/plugin-manifest.schema.json) za pomocą crate'a `jsonschema` dodanego jako dev-dependency (test `manifest_is_valid_against_the_schema`). Nada się dowolny walidator JSON Schema w wersji draft 2020-12. Pełna walidacja, obejmująca reguły dotyczące wielu pól naraz, odbywa się podczas [pakowania](../packaging/#polecenie-package) wtyczki.

## MockHost

`MockHost` odpowiada na żądania funkcji hosta (capability) tak, jak zrobiłby to agent, i je rejestruje.

| Konstruktor | Zachowanie |
| --- | --- |
| `MockHost::allow_all()` | Każde żądanie kończy się powodzeniem z wartością `null`: pusta odpowiedź HTTP 200 dla pobrań, uruchomienie rozpoczęte z aplikacji dla kontekstu uruchomienia |
| `MockHost::deny_all()` | Każde żądanie jest odrzucane tak jak przy nieprzyznanym uprawnieniu |
| `MockHost::new(closure)` | Domknięcie otrzymuje każde `CapabilityRequest` i zwraca `CallResult` |

| Metoda | Działanie |
| --- | --- |
| `host.invoke(&plugin, action, json_params)` | Wykonuje jedno wywołanie tak, jak zrobiłby to host: init, sprawdzenie parametrów, procedura obsługi, sprawdzenie wyniku. Zwraca `ActionOutcome` |
| `host.requests()` | Dotychczasowe żądania, po kolei |
| `host.context()` | `Context`, którego wywołania trafiają do tej atrapy; służy do bezpośredniego testowania funkcji pomocniczej |

### Czysta akcja

Akcja, która nie potrzebuje żadnej funkcji hosta, musi działać, gdy wszystko jest zabronione. Ten test pochodzi z przykładu Hello World:

```rust
    #[test]
    fn greet_asks_for_nothing() {
        // Every capability denied: greet must still work.
        let mut host = MockHost::deny_all();
        let out = host.invoke(&plugin(), "greet", json!({"name": "  Ada "}));
        let ActionOutcome::Ok { message, data } = out else {
            panic!("{out:?}")
        };
        assert_eq!(message.as_deref(), Some("Hello, Ada!"));
        assert_eq!(data["greeting"], ParamValue::String("Hello, Ada!".into()));
        assert!(host.requests().is_empty());
    }
```

### Skryptowanie odpowiedzi agenta

Dla akcji korzystających z funkcji hosta przygotuj odpowiedzi w skrypcie. Ten host ma jedno ustawienie, zapisane dane uwierzytelniające i gotową odpowiedź HTTP:

```rust
/// A scripted agent: a stored token, one setting and a canned API answer.
fn status_host(status: u16, body: Value) -> MockHost {
    MockHost::new(move |request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(json!({ "defaultService": "api" })),
        CapabilityRequest::CredentialHas(_) => CallResult::Ok(json!(true)),
        CapabilityRequest::HttpFetch(_) => CallResult::Ok(json!({
            "status": status,
            "headers": [{ "name": "date", "value": "Mon, 16 Nov 2026 10:00:00 GMT" }],
            "body": body.to_string(),
        })),
        _ => CallResult::Ok(Value::Null),
    })
}
```

Następnie sprawdź asercjami wynik oraz to, o co wtyczka poprosiła:

```rust
#[test]
fn a_missing_service_is_a_typed_error() {
    let mut host = status_host(404, json!({}));
    let out = host.invoke(&plugin(), "check", json!({ "service": "nope" }));
    assert!(
        matches!(&out, ActionOutcome::Failed { code, .. } if code == "service-not-found"),
        "{out:?}"
    );
}
```

### Odmowy

Przetestuj, czy odmowa dostępu do funkcji hosta zamienia się w czytelny błąd, tak jak stanie się to po cofnięciu uprawnienia przez użytkownika:

```rust
#[test]
fn a_denied_request_is_not_permitted() {
    // Settings and the credential answer; the fetch is denied like a revoked grant.
    let mut host = MockHost::new(|request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(json!({})),
        CapabilityRequest::CredentialHas(_) => CallResult::Ok(json!(true)),
        _ => CallResult::error(CallErrorCode::NotPermitted, "not granted"),
    });
    let out = host.invoke(&plugin(), "check", json!({ "service": "api" }));
    assert!(
        matches!(&out, ActionOutcome::Failed { code, .. } if code == codes::NOT_PERMITTED),
        "{out:?}"
    );
}
```

### Czego atrapa nie sprawdza

Atrapa hosta nie jest brokerem uprawnień. Nie zna twojego manifestu, więc nie odrzuca niezadeklarowanego hosta, niezadeklarowanego identyfikatora zmiennej ani danych wyjściowych, których manifest nie wymienia, i nie stosuje żadnych limitów częstości. Te kontrole odbywają się w agencie. Rodzaje parametrów istniejące tylko w SDK (`Param::number`, `Param::secret`) również działają z atrapą, ale nie przez agenta.

## Z prawdziwym agentem

Repozytorium DeckPilota dokumentuje ręczne sprawdzenie całej ścieżki na macOS lub Linuksie w [docs/marketplace/local-testing.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/local-testing.md). W zarysie:

1. **Zbuduj** agenta z funkcją Cargo `plugins`, narzędzie rejestru, hosta wtyczek i moduł:

   ```sh
   cargo build -p deckpilot-agent --features plugins
   cargo build -p deckpilot-plugin-registry --features publish --bin deckpilot-registry
   (cd plugins/runtime &&
     cargo build --locked -p deckpilot-plugin-host --bin deckpilot-plugin-host &&
     cargo build --locked -p hello-weather --target wasm32-unknown-unknown --profile guest)
   ```

2. **Utwórz lokalny rejestr** z jednorazowymi kluczami. Skrypt z przewodnika generuje klucz główny (root), klucz indeksu i klucz znacznika czasu (timestamp) poleceniem `deckpilot-registry keys generate`, inicjalizuje i podpisuje zestaw kluczy, dodaje wydawcę, pakuje wtyczkę, a następnie uruchamia `add-version`, `build-index`, `sign-index`, `timestamp` i `verify-site`.
3. **Uruchom agenta**, wskazując hosta, lokalny rejestr i jego klucze główne. Agent zbudowany z funkcją `plugins` ma włączone wtyczki, chyba że zostanie uruchomiony z flagą `--disable-plugins` lub ze zmienną `DECKPILOT_ENABLE_PLUGINS=0`:

   ```sh
   target/debug/deckpilot-agent --headless --no-usb \
     --plugin-host "$DECKPILOT_PLUGIN_HOST" \
     --plugin-registry "$REG" --plugin-registry-roots "$KEYS/roots.json"
   ```

   Ścieżka do hosta musi być bezwzględna. Aplikacja pokazuje stronę Plugins tylko wtedy, gdy została uruchomiona z `VITE_DECKPILOT_PLUGINS=1`.
4. **Przejdź** w aplikacji przez instalację, zgodę, włączenie, uruchomienie, cofnięcie uprawnienia, aktualizację z dodanym uprawnieniem, przywrócenie poprzedniej wersji i odinstalowanie.

Aby przetestować w ten sposób własną wtyczkę, zastąp katalog i moduł przykładu swoimi. Utworzone tutaj klucze służą wyłącznie do użytku lokalnego; nie mają nic wspólnego z publicznym rejestrem.

Ten sam przebieg jest zautomatyzowany dla przykładowych wtyczek w `apps/agent/tests/plugins_examples.rs`, z gotowymi odpowiedziami HTTP:

```sh
cargo test -p deckpilot-agent --features plugins --test plugins_examples --locked -- --nocapture
```

## Lista kontrolna przed pakowaniem

- `manifest_mismatches` zwraca pustą listę, a manifest przechodzi walidację.
- Każda akcja ma test z `deny_all()` albo z hostem o skryptowanych odpowiedziach, w tym test odmowy.
- Każdy własny kod błędu jest zadeklarowany.
- Każdy klucz danych wyjściowych ustawiany przez kod jest zadeklarowany w `node.outputs`.
- Żadne dane wejściowe nie mogą wywołać paniki.

## Dokumentacja źródłowa

Sekcja Testing w [docs/marketplace/sdk/building.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/building.md) oraz [docs/marketplace/local-testing.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/local-testing.md).
