---
title: "Przegląd SDK"
description: "Typy crate'a Rusta deckpilot-plugin-sdk: Plugin, Action, Param, Context, Args, Output, makro plugin! oraz MockHost do testów."
group: start
order: 3
---

`deckpilot-plugin-sdk` to crate Rusta, za pomocą którego piszesz wtyczkę. Deklaruje wtyczkę i jej akcje, sprawdza parametry, zanim uruchomi się twój kod, opakowuje każdą funkcję hosta (capability) z ABI gościa 1 w typowaną metodę i generuje eksporty WebAssembly. Ta strona jest mapą crate'a; pozostałe strony omawiają szczegółowo każdą jego część.

> **Status:** Crate ma wersję 0.1.0, implementuje API wtyczek 1.0 i ABI gościa 1 i nie jest opublikowany w crates.io. Jak dodać go jako zależność, opisuje strona [Pierwsze kroki](../getting-started/#skąd-wziąć-sdk). Dopóki ma wersję 0.x, wersja minor może zmienić API w Ruście.

## Cała wtyczka

Ten przykład z dokumentacji samego crate'a pokazuje większość jego powierzchni: definicję, dwa typowane parametry, handler, wiersz logu i ustrukturyzowany wynik.

```rust
use deckpilot_plugin_sdk::prelude::*;

fn plugin() -> Plugin {
    Plugin::new("acme.hello", "0.1.0").action(
        Action::new("greet", greet)
            .param(Param::text("name").required().max_length(64))
            .param(Param::choice("style", &["plain", "excited"]).default("plain")),
    )
}

fn greet(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let name: String = args.get("name")?;
    let style: String = args.get("style")?;
    ctx.info("greeting someone");
    let mark = if style == "excited" { "!" } else { "." };
    Ok(Output::message(format!("Hello, {name}{mark}")).with("name", name))
}
```

W prawdziwym cracie wtyczki eksporty generuje jeszcze jeden wiersz: `deckpilot_plugin_sdk::plugin!(plugin);`.

## Prelude

`use deckpilot_plugin_sdk::prelude::*;` wprowadza wszystko, czego wtyczka zwykle potrzebuje:

| Typ | Czym jest |
| --- | --- |
| `Plugin` | Definicja: identyfikator, wersja, akcje i dwa haki cyklu życia |
| `Action` | Jedna akcja: identyfikator z manifestu, jej parametry i handler |
| `Param` | Jeden typowany parametr z ograniczeniami, opcjami i wartością domyślną |
| `Context` | Uchwyt do wywołań funkcji hosta wewnątrz handlera |
| `Args` | Sprawdzone parametry jednego wywołania |
| `Output` | Wynik udany albo pominięty, z komunikatem i nazwanymi wyjściami |
| `ActionResult` | `Result<Output, PluginError>`, czyli to, co zwraca handler |
| `PluginError` | Typowany błąd: kod, komunikat, opcjonalnie z możliwością ponowienia |
| `codes` | Wspólne kody błędów jako stałe |
| `Request`, `Response`, `HttpMethod` | HTTP za pośrednictwem brokera uprawnień |
| `SecretHandle`, `AuthScheme` | Pola danych uwierzytelniających (sloty), wyłącznie po nazwie |
| `Settings`, `StorageUsage` | To, co zwracają `Context::settings` i `Context::storage_usage` |
| `CapabilityError`, `CapabilityErrorKind`, `NetworkErrorKind` | Dlaczego wywołanie funkcji hosta się nie powiodło |
| `RunContext`, `RunTrigger`, `RunCause` | Co rozpoczęło bieżące uruchomienie |
| `LogLevel`, `NotifySeverity`, `ActionOutcome` | Poziomy logowania, wygląd powiadomień, postać wyniku przesyłana w protokole |

## Definiowanie wtyczki

`Plugin::new(id, version)` przyjmuje ten sam identyfikator i tę samą wersję co `manifest.json`. Dla każdej akcji dołącz w łańcuchu `.action(Action)`, a opcjonalnie także `.on_init(fn)` i `.on_shutdown(fn)`.

`Action::new(id, handler)` przyjmuje handler typu `fn(&mut Context<'_>, &Args<'_>) -> ActionResult`: zwykłą funkcję albo domknięcie, które niczego nie przechwytuje. `.param(Param)` dodaje parametr.

`Plugin::validate()` sprawdza identyfikatory, klucze, unikalność, rodzaje i wartości domyślne względem reguł manifestu. Wywołaj ją w teście jednostkowym. Przy niepoprawnej definicji każde wywołanie kończy się błędem z kodem `plugin-error`.

Szczegóły: [Akcje](../actions/) i [Parametry i ustawienia](../parameters/).

## Makro `plugin!`

`deckpilot_plugin_sdk::plugin!(build_fn)` generuje eksporty ABI gościa 1: `dp_abi_version`, `dp_alloc`, `dp_invoke`, `dp_describe`, `dp_init`, `dp_shutdown` i `dp_features`. Użyj go raz, w korzeniu crate'a `cdylib`, z funkcją typu `fn() -> Plugin`. Wtyczka jest budowana przy pierwszym użyciu i przechowywana przez cały czas życia modułu.

Twój crate nie potrzebuje własnego `unsafe`. Użyj `#![deny(unsafe_code)]` zamiast `forbid`, ponieważ atrybuty eksportów w makrze wymagają `allow`.

## Funkcje hosta w `Context`

Każda z poniższych metod to jedno żądanie funkcji hosta, które rozstrzyga broker uprawnień agenta. Na jedno wywołanie akcji dozwolonych jest co najwyżej 256 wywołań.

| Funkcja hosta | Metody | Uprawnienie |
| --- | --- | --- |
| HTTP | `fetch(Request)`, `http_get(url)` | `network` dla dokładnego hosta i metody |
| Dane uwierzytelniające | `has_credential(&SecretHandle)`; `Request::bearer`, `Request::credential` | `credentials` dla slotu |
| Ustawienia | `settings()` | brak |
| Pamięć trwała | `storage_get`, `storage_get_as::<T>`, `storage_set`, `storage_delete`, `storage_keys`, `storage_usage` | brak (uprawnienie `storage` podnosi limit) |
| Zmienne | `publish_variable`, `clear_variable`, `clear_variables` | brak, dla własnych zadeklarowanych zmiennych wtyczki |
| Powiadomienia | `notify(title, body)`, `notify_with(severity, title, body)` | `notifications` |
| Otwieranie adresu URL | `open_url(url)` | `openUrl` dla dokładnego hosta |
| Kontekst uruchomienia | `run_context()` | brak |
| Log | `log(level, message)`, `debug`, `info`, `warn`, `error` | brak |

Każda zwraca `Result<_, CapabilityError>`, z wyjątkiem metod logowania, które działają na zasadzie best effort i niczego nie zwracają. W handlerze `?` zamienia `CapabilityError` na `PluginError` ze wspólnym kodem, takim jak `not-permitted` lub `request-failed`.

Szczegóły: [Uprawnienia](../permissions/), [Dane uwierzytelniające](../credentials/), [Pamięć trwała](../storage/), [Zmienne](../variables/).

## Wyniki i błędy

Handler zwraca `Ok(Output)` albo `Err(PluginError)`. `Output::ok()`, `Output::message(text)` i `Output::skipped(text)` tworzą wynik, a `.with(key, value)` dodaje nazwane wyjście. `PluginError::new(code, message)` tworzy błąd, a `.retryable()` oznacza go jako przejściowy. Zobacz [Akcje](../actions/#wyniki).

## Wsparcie testowania

Moduł `testing` uruchamia akcje natywnie, bez WebAssembly i bez agenta:

- `testing::MockHost` odpowiada na żądania funkcji hosta za pomocą domknięcia i je rejestruje. `MockHost::allow_all()` i `MockHost::deny_all()` to gotowe atrapy hosta.
- `testing::manifest_mismatches(&plugin, &manifest)` wymienia każdą różnicę między kodem a `manifest.json`.

Poza WebAssembly wywołanie funkcji hosta, które nie przechodzi przez `MockHost`, kończy się błędem `CapabilityErrorKind::Unavailable`. Zobacz [Testowanie](../testing/).

## Stałe wersji

| Stała | Wartość | Znaczenie |
| --- | --- | --- |
| `deckpilot_plugin_sdk::SDK_VERSION` | `0.1.0` | Wersja samego crate'a |
| `deckpilot_plugin_sdk::API_LEVEL` | 1.0 | Poziom API wtyczek; manifest deklaruje `"pluginApi": "^1.0"` |
| `deckpilot_plugin_sdk::ABI_VERSION` | 1 | ABI gościa, sprawdzane asercją w czasie kompilacji |

Zobacz [Zgodność wersji](../compatibility/).

## Czego SDK nie oferuje

ABI 1 nie daje dostępu do systemu plików, procesów, schowka ani powłoki, nie ma zegara ani źródła losowości. `std::time::SystemTime::now()` i generatory liczb losowych w module zwracają błąd albo panikują. Nie ma timerów, pracy w tle, strumieniowego HTTP ani WebSocketów, a wtyczka nie może odczytywać zmiennych, aktywnego profilu ani projektu.

SDK deklaruje też `Param::number` i `Param::secret`. Wersja manifestu 1 nie ma dla nich rodzaju parametru, więc działają pod `MockHost`, ale nie da się do nich dotrzeć przez agenta; zobacz [Parametry i ustawienia](../parameters/#rodzaje-których-nie-ma-w-manifeście).

## JavaScript

Repozytorium zawiera też `plugins/sdk-js`, czyli pakiet `@deckpilot/plugin-sdk` w wersji 0.1.0. Jest prywatny, nigdy nie został opublikowany i zawiera wyłącznie deklaracje typów. Żaden proces hosta DeckPilota nie potrafi uruchomić wtyczki w JavaScripcie: kompilacja JavaScriptu do modułu ABI 1 nie jest dostępna. Deklaracje opisują wcześniejszy projekt i w kilku miejscach różnią się od ABI 1; różnice wymienia [README tego pakietu](https://github.com/knittingcode/deckpilot/blob/main/plugins/sdk-js/README.md). Pisz wtyczki w Ruście.

## Dokumentacja źródłowa

Pełna dokumentacja SDK, w tym tabela pokrycia każdej funkcji hosta wraz z jej testami, to [docs/marketplace/sdk/README.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/README.md). Kod źródłowy crate'a znajduje się w [plugins/runtime/guest/plugin-sdk](https://github.com/knittingcode/deckpilot/blob/main/plugins/runtime/guest/plugin-sdk/src/lib.rs).
