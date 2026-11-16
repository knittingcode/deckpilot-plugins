---
title: "Zmienne"
description: "Deklarowanie zmiennych w manifeście, publikowanie i czyszczenie ich z akcji oraz sposób, w jaki odczytują je workflow i przyciski."
group: capabilities
order: 8
---

Wtyczka może publikować nazwane wartości, takie jak status builda albo licznik, które workflow i przyciski odczytują za pomocą szablonu. Wtyczka deklaruje swoje zmienne w manifeście i ustawia je z akcji. Publikowanie nie wymaga uprawnienia, ponieważ wtyczka może publikować tylko zmienne, które sama zadeklarowała.

## Deklarowanie zmiennych

Każda zmienna to wpis w `contributes.variables`:

```json
"variables": [
  {
    "id": "status",
    "name": "Service status",
    "description": "The status the last check found, for example up or degraded.",
    "valueType": { "type": "string" },
    "scope": "machine"
  },
  {
    "id": "healthy",
    "name": "Service healthy",
    "valueType": { "type": "boolean" },
    "scope": "machine"
  }
]
```

| Pole | Reguła |
| --- | --- |
| `id` | Ścieżka względna: `[a-z][A-Za-z0-9_]{0,31}` oraz maksymalnie dwa kolejne segmenty oddzielone kropkami, co najwyżej 100 znaków, unikalna w obrębie wtyczki |
| `name` | Od 1 do 48 znaków |
| `description` | Opcjonalne, co najwyżej 300 znaków |
| `valueType` | `{ "type": "string" }`, `"integer"`, `"boolean"` lub `"json"` |
| `scope` | `machine`, `profile` lub `project` |
| `requires`, `sensitive`, `deprecated` | Opcjonalne |

Wtyczka deklaruje co najwyżej 64 zmienne.

> **Uwaga:** Schemat akceptuje trzy zakresy, ale w tej wersji można publikować tylko `machine`. Publikacja zmiennej zadeklarowanej z zakresem `profile` albo `project` kończy się błędem rodzaju `Unavailable`.

## Publikowanie

Wewnątrz handlera zmienną ustawia `Context::publish_variable(id, value)`. Wartością może być wszystko, co serializuje się do zadeklarowanego typu:

```rust
    ctx.publish_variable("status", &last.status)?;
    ctx.publish_variable("healthy", healthy)?;
```

Agent sprawdza każde wywołanie:

| Sytuacja | Rodzaj błędu |
| --- | --- |
| Identyfikator nie jest zadeklarowany w manifeście | `NotPermitted` |
| Wartość nie ma zadeklarowanego `valueType` | `BadRequest` |
| Zakres zmiennej jest inny niż `machine` | `Unavailable` |

Limity: 10 wywołań publikacji lub czyszczenia na sekundę, wartość o rozmiarze co najwyżej 16 KiB i identyfikator o długości co najwyżej 100 znaków.

## Czyszczenie

Wyczyszczona zmienna nie ma wartości. `clear_variable(id)` czyści jedną zmienną, `clear_variables()` czyści wszystkie zmienne opublikowane przez wtyczkę, a opublikowanie `null` również czyści zmienną.

```rust
fn reset(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    ctx.storage_delete(LAST_CHECK)?;
    ctx.clear_variables()?;
    Ok(Output::message("forgot the last check"))
}
```

## Jak odczytywane są zmienne

Workflow i przyciski odczytują zmienną wtyczki za pomocą szablonu w postaci:

```text
{{ plugins.<publisherCamel>.<nameCamel>.<id> }}
```

Wydawca i nazwa wtyczki są zapisywane w notacji lower camel case. Dla wtyczki `acme.status-check` i zmiennej `status` wygląda to tak:

```text
{{ plugins.acme.statusCheck.status }}
```

Reguły identyfikatora wtyczki sprawiają, że to odwzorowanie jest jednoznaczne: w identyfikatorze po łączniku zawsze następuje litera.

Agent rozwiązuje szablon na podstawie ostatnio opublikowanej wartości i ogłasza aplikacji każdą zmianę. Przy odczycie zmiennej nie uruchamia się żaden kod wtyczki.

## Zasady odczytu dla wtyczek

W ABI 1 wtyczka nie może odczytywać zmiennych: ani własnych, ani zmiennych innych wtyczek, ani zmiennych DeckPilota. Nie ma do tego funkcji hosta (capability). Schemat manifestu definiuje już uprawnienie `variables` z listą `read` dla zmiennych innych właścicieli, ale na razie na nic ono nie pozwala; zobacz [Uprawnienia](../permissions/#rodzaje-uprawnień).

Jeśli akcja potrzebuje wartości, którą opublikowała wcześniej, przechowuj jej kopię w [pamięci trwałej](../storage/).

## Czas życia opublikowanych wartości

Opublikowane wartości żyją wyłącznie w pamięci agenta.

| Zdarzenie | Opublikowane wartości |
| --- | --- |
| Agent uruchamia się ponownie albo konfiguracja zostaje zablokowana i odblokowana | usuwane |
| Wtyczka zostaje wyłączona, zaktualizowana, przywrócona do poprzedniej wersji, odinstalowana albo poddana kwarantannie | usuwane |
| Uprawnienie zostaje cofnięte albo przyznane | zachowywane |
| Proces hosta wtyczki ulega awarii | zachowywane |

Ponieważ wtyczka działa tylko wtedy, gdy wykonuje się jedna z jej akcji, nie może sama opublikować wartości ponownie po restarcie. To, co musi przetrwać, przechowuj w pamięci trwałej i opublikuj ponownie przy następnym uruchomieniu akcji. Przykład robi dokładnie to: `check` zapisuje wynik i go publikuje, a `last-check` może odczytać zapisaną kopię bez wykonywania żądania.

## Testowanie

Pod atrapą hosta wywołanie publikacji jest rejestrowane jako `CapabilityRequest::VariablesPublish` z polami `id` i `value`, więc test może sprawdzić asercją, co zostało opublikowane:

```rust
    assert!(host.requests().iter().any(|r| matches!(r,
        CapabilityRequest::VariablesPublish(v) if v.id == "status" && v.value == "up")));
```

Atrapa hosta nie sprawdza identyfikatorów względem manifestu. Ta kontrola odbywa się w agencie. Zobacz [Testowanie](../testing/).

## Dokumentacja źródłowa

Sekcja „Variables” w [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md) oraz tabela zmiennych w [docs/marketplace/sdk/manifest.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/manifest.md).
