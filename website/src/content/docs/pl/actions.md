---
title: "Akcje"
description: "Deklaruj akcje wtyczki w Ruście i w manifest.json, zwracaj wyjścia i typowane błędy oraz ustawiaj metadane węzła, takie jak limit czasu i efekt."
group: build
order: 5
---

Akcja to jednostka pracy, którą oferuje wtyczka: coś, co może wywołać przycisk urządzenia, węzeł workflow albo przycisk uruchamiania w aplikacji. Każda akcja jest deklarowana dwukrotnie: raz w `manifest.json`, dla agenta, edytorów i użytkownika, i raz w kodzie, który określa, co faktycznie się wykonuje. Obie deklaracje muszą być ze sobą zgodne.

## Deklarowanie akcji w kodzie

Definicja wtyczki wymienia akcje wraz z ich parametrami. Oto przykład używany na tych stronach, wtyczka, która sprawdza usługę w API statusu:

```rust
pub fn plugin() -> Plugin {
    Plugin::new("acme.status-check", "0.1.0")
        .on_init(init)
        .on_shutdown(shutdown)
        .action(
            Action::new("check", check)
                .param(Param::text("service").max_length(64))
                .param(Param::integer("minUptime").min(0.0).max(100.0).default(99))
                .param(Param::boolean("notify").default(false)),
        )
        .action(Action::new("last-check", last_check))
        .action(
            Action::new("open-page", open_page)
                .param(Param::text("service").required().max_length(64)),
        )
        .action(Action::new("reset", reset))
}

deckpilot_plugin_sdk::plugin!(plugin);
```

- Argumenty `Plugin::new` są równe polom `id` i `version` manifestu.
- Pierwszy argument `Action::new` jest równy `contributes.actions[].id`.
- Handler ma typ `fn(&mut Context<'_>, &Args<'_>) -> ActionResult`.
- Wtyczka ma co najwyżej 64 akcje, a akcja co najwyżej 32 parametry.

## Deklarowanie akcji w manifeście

Odpowiadający jej wpis w manifeście dodaje to, co widzą użytkownicy i edytory: nazwę, opis, kategorię, etykiety parametrów i metadane węzła.

```json
{
  "id": "last-check",
  "name": "Last check",
  "description": "Shows the result of the last check from the plugin's storage, without a request.",
  "category": "Status Check",
  "parameters": [],
  "node": {
    "version": 1,
    "outputs": [
      { "key": "service", "label": "Service", "kind": "text" },
      { "key": "status", "label": "Status", "kind": "text" },
      { "key": "checkedAt", "label": "Checked at", "kind": "text" }
    ],
    "timeoutMs": 5000,
    "retrySafe": true,
    "idempotent": true,
    "effect": "readOnly"
  }
}
```

Agent rejestruje akcję jako typ `<plugin id>.<action id>`, tutaj `acme.status-check.last-check`. Reguły dotyczące pól podaje strona [Format manifestu](../manifest/#akcje).

## Handlery

Handler odczytuje swoje parametry z `Args`, sięga do świata zewnętrznego przez `Context` i zwraca wynik. Ten odczytuje pamięć trwałą wtyczki i nie potrzebuje żadnego uprawnienia:

```rust
fn last_check(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let Some(last) = ctx.storage_get_as::<LastCheck>(LAST_CHECK)? else {
        return Ok(Output::skipped("nothing checked yet"));
    };
    Ok(Output::message(format!("{}: {}", last.service, last.status))
        .with("service", last.service)
        .with("status", last.status)
        .with("checkedAt", last.checked_at))
}
```

Zanim handler się uruchomi, parametry zostały zwalidowane dwukrotnie: przez agenta względem manifestu i przez SDK względem deklaracji `Param`. Zobacz [Parametry i ustawienia](../parameters/#walidacja).

## Wyniki

Handler zwraca `ActionResult`, czyli `Result<Output, PluginError>`.

| Konstruktor | Rezultat | Znaczenie |
| --- | --- | --- |
| `Output::ok()` | `ok` | Sukces, bez komunikatu |
| `Output::message(text)` | `ok` | Sukces z komunikatem dla użytkownika, co najwyżej 1024 znaki |
| `Output::skipped(text)` | `skipped` | Nie było nic do zrobienia; krok workflow liczy się jako pominięty |
| `.with(key, value)` | | Dodaje jedno nazwane wyjście |
| `Err(PluginError::new(code, message))` | `failed` | Typowany błąd; `.retryable()` oznacza go jako przejściowy |

`.with` przyjmuje ciągi znaków, wartości logiczne, liczby całkowite (od `i8` do `i64`, od `u8` do `u32`), `f32` i `f64` oraz `Vec` takich wartości.

Zanim wynik opuści moduł, SDK sprawdza go względem ograniczeń ABI 1: co najwyżej 64 wyjścia, nazwy o długości co najwyżej 64 znaków, wartości tekstowe o rozmiarze co najwyżej 16 KiB, listy o co najwyżej 256 elementach, zagnieżdżenie na co najwyżej 4 poziomy i liczby skończone. Całe wyjście to co najwyżej 192 KiB JSON-a. Wynik wykraczający poza te ograniczenia staje się błędem z kodem `invalid-result`.

Nigdy nie umieszczaj sekretu w komunikacie ani w wyjściu.

## Wyjścia

Wyjście to nazwana wartość, której workflow może użyć po wykonaniu akcji. Każde zadeklaruj w `node.outputs`:

| Pole | Reguła |
| --- | --- |
| `key` | camelCase, `[a-z][A-Za-z0-9]{0,31}`; klucz przekazywany do `.with` |
| `label` | Od 1 do 48 znaków |
| `kind` | `text` (wartość domyślna), `integer`, `boolean`, `path`, `url` lub `projectKey` |
| `always` | Czy ustawia je każde udane uruchomienie |
| `sensitive` | Oznacza wartość jako wrażliwą |

Agent zachowuje tylko zadeklarowane wyjścia. Wszystko inne, co akcja zwróci przez `.with`, jest odrzucane. Akcja ma co najwyżej 32 wyjścia. W workflow każda wartość wyjścia staje się tekstem; przycisk uruchamiania w aplikacji otrzymuje je jako JSON.

## Metadane węzła

Opcjonalny obiekt `node` mówi silnikowi workflow, jak zachowuje się akcja.

| Pole | Wartości | Znaczenie |
| --- | --- | --- |
| `version` | liczba całkowita, co najmniej 1 | Wersja kontraktu węzła |
| `icon` | `[a-z0-9-]{1,64}` | Nazwa ikony w edytorach |
| `outputs` | co najwyżej 32 | Zobacz wyżej |
| `timeoutMs` | od 1000 do 600 000; domyślnie 30 000 | Limit czasu rzeczywistego jednego wywołania. Gdy upłynie, proces hosta jest zatrzymywany, a akcja kończy się błędem |
| `retrySafe` | wartość logiczna, domyślnie `false` | Ponowienie nie może wyrządzić szkody. Błąd z możliwością ponowienia jest honorowany tylko dla akcji, które ustawiają to pole |
| `idempotent` | wartość logiczna, domyślnie `false` | Dwukrotne uruchomienie ma taki sam skutek jak jednokrotne |
| `effect` | `readOnly` (domyślnie), `opens`, `modifies` | Co akcja robi ze światem zewnętrznym. Walidacja ostrzega, gdy akcja `modifies` ma ustawione `retrySafe` |
| `cancellable` | wartość logiczna | Akceptowane; agent traktuje każdą akcję wtyczki jako możliwą do anulowania |

Dobierz limit czasu uczciwie. Akcja, która wykonuje jedno żądanie HTTP, potrzebuje niewiele więcej niż 15 sekund, na które agent pozwala jednemu żądaniu.

## Kody błędów

Nieudana akcja niesie kod, komunikat o długości co najwyżej 1024 znaków oraz informację, czy ponowienie może pomóc. Kod pasuje do `[a-z][a-z0-9]*(-[a-z0-9]+)*` i ma co najwyżej 64 znaki.

### Kody wspólne

Kody wspólne można zwracać bez deklarowania i nie wolno deklarować ich ponownie. Najczęstsze z nich SDK udostępnia jako stałe w `codes`:

| Kod | Stała | Typowe źródło |
| --- | --- | --- |
| `not-permitted` | `NOT_PERMITTED` | Funkcja hosta (capability), na którą użytkownik nie przyznał uprawnienia, przez `?` |
| `invalid-parameters` | `INVALID_PARAMETERS` | Kontrola parametrów w agencie albo w SDK; dane wejściowe, których rodzaje nie potrafią wyrazić |
| `credential-missing` | `CREDENTIAL_MISSING` | Pole danych uwierzytelniających (slot) bez zapisanego sekretu |
| `request-failed` | `REQUEST_FAILED` | Nieudane wywołanie funkcji hosta, przez `?` |
| `throttled` | `THROTTLED` | Limit częstości, lokalny albo zdalny |
| `storage-quota` | `STORAGE_QUOTA` | Limit pamięci trwałej |
| `timeout` | `TIMEOUT` | Przekroczenie limitu czasu po stronie zdalnej |
| `unsupported` | `UNSUPPORTED` | Nieznany identyfikator akcji albo nieznana operacja |
| `plugin-error` | `PLUGIN_ERROR` | Błąd w kodzie: niepoprawna definicja, nieudany init |
| `invalid-result` | `INVALID_RESULT` | Wynik wykraczający poza ograniczenia ABI 1 |

Nieudane żądanie HTTP może też zgłosić jeden z kodów sieciowych: `blocked-address`, `dns-failed`, `connection-failed`, `tls-failed`, `too-many-redirects` albo `insecure-redirect`. Pełna lista to `codes::SHARED`.

Agent dodaje własne kody dla przycisków i węzłów workflow, między innymi `cancelled`, `plugin-host-failed`, `plugin-busy`, `plugin-revoked` i `plugin-unavailable`.

### Własne kody

Mapuj zdalne błędy na kody, na które użytkownik może zareagować, i każdy z nich zadeklaruj w `contributes.errorCodes`:

```rust
    match response.status {
        200..=299 => Ok(response),
        404 => Err(PluginError::new(
            "service-not-found",
            "the status API does not know this service",
        )),
        429 => Err(PluginError::new(codes::THROTTLED, "the status API is rate limiting").retryable()),
        s @ 500..=599 => Err(PluginError::new(
            "status-api-error",
            format!("the status API failed (HTTP {s})"),
        )
        .retryable()),
        s => Err(PluginError::new(
            "status-api-error",
            format!("the status API answered HTTP {s}"),
        )),
    }
```

```json
"errorCodes": {
  "service-required": "No service was named and the settings have no default service.",
  "invalid-service": "The service name contains characters the status API does not use.",
  "service-not-found": "The status API does not know this service.",
  "status-api-error": "The status API answered with an error status.",
  "invalid-response": "The status API answered with something that is not a status."
}
```

Deklaracja dokumentuje kod dla użytkowników i osób przeprowadzających przegląd. Ta wersja agenta nie odrzuca w czasie działania kodu niezadeklarowanego, a jedynie kod źle sformowany. Test jednostkowy, który porównuje kody zwracane przez wtyczkę z manifestem, utrzymuje zgodność jednego z drugim; zobacz [Testowanie](../testing/).

Nie dopuszczaj do paniki (panic). Panika kończy proces hosta, akcja kończy się błędem `plugin-host-failed`, a powtarzające się awarie wyłączają wtyczkę; zobacz [Cykl życia](../lifecycle/).

## Dokumentacja źródłowa

[docs/marketplace/sdk/actions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/actions.md) oraz [docs/marketplace/sdk/errors-and-versions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/errors-and-versions.md).
