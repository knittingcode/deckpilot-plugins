---
title: "Parametry i ustawienia"
description: "Rodzaje parametrów, wartości wymagane i domyślne, dwa etapy walidacji, odczytywanie wartości przez Args oraz ustawienia wtyczki."
group: build
order: 6
---

Parametry to typowane dane wejściowe akcji. Użytkownik wypełnia je, konfigurując przycisk albo węzeł workflow, agent je waliduje, a handler odczytuje je z `Args`. Ustawienia używają tych samych rodzajów, ale należą do wtyczki jako całości.

## Deklarowanie parametru

W manifeście parametr jest obiektem w `contributes.actions[].parameters`:

```json
{
  "key": "minUptime",
  "label": "Minimum uptime (%)",
  "required": false,
  "kind": { "type": "integer", "min": 0, "max": 100 },
  "default": { "type": "integer", "value": 99 }
}
```

| Pole | Reguła |
| --- | --- |
| `key` | camelCase, `[a-z][A-Za-z0-9]{0,31}` |
| `label` | Od 1 do 48 znaków |
| `description` | Opcjonalne, co najwyżej 300 znaków |
| `kind` | Obiekt z polem `type` i polami danego rodzaju |
| `required` | Wartość logiczna, zawsze obecna |
| `default` | Tylko dla parametrów opcjonalnych: `{ "type": "text" \| "integer" \| "boolean" \| "choice", "value": … }` |

W kodzie ten sam parametr to `Param` w akcji:

```rust
            Action::new("check", check)
                .param(Param::text("service").max_length(64))
                .param(Param::integer("minUptime").min(0.0).max(100.0).default(99))
                .param(Param::boolean("notify").default(false)),
```

Parametr jest opcjonalny, chyba że wywołano `.required()`. Wartość domyślna jest dozwolona tylko dla parametrów opcjonalnych, a `Plugin::validate()` sprawdza ją względem rodzaju.

## Rodzaje

| `kind.type` | Pola rodzaju | Dociera jako | Deklaracja w SDK | Odczyt jako |
| --- | --- | --- | --- | --- |
| `text` | `maxLength` (od 1 do 100 000), `multiline` | ciąg znaków | `Param::text(key)`, `.max_length(n)` | `String` |
| `integer` | `min`, `max` | liczba całkowita | `Param::integer(key)`, `.min(a)`, `.max(b)` | `i64` |
| `boolean` | | wartość logiczna | `Param::boolean(key)` | `bool` |
| `choice` | `options` (od 1 do 64 unikalnych opcji o długości od 1 do 64 znaków) | ciąg znaków | `Param::choice(key, &[..])` | `String` |
| `project` | | identyfikator projektu, jako ciąg znaków | `Param::text(key)` | `String` |
| `lines` | `itemLabel`, `maxItems`, `maxLength` | tekst | `Param::text(key)` | `String` |
| `pairs` | `separator` (`=` lub `:`), `nameLabel`, `valueLabel`, `maxItems`, `caseInsensitive`, `noSecrets` | tekst | `Param::text(key)` | `String` |
| `code` | `language` (`json`, `text`, `yaml`, `markdown`), `maxLength` | tekst | `Param::text(key)` | `String` |

Cztery ostatnie rodzaje dają użytkownikowi bogatszy edytor, ale do wtyczki docierają jako tekst, więc w kodzie deklaruje się je jako `Param::text`.

Jedna wartość tekstowa ma w ABI 1 co najwyżej 16 KiB w UTF-8. Dlatego przy `maxLength` powyżej 4096 znaków tekst spoza ASCII może zostać odrzucony przez proces hosta, a nie przez bardziej przyjazną kontrolę agenta.

### Rodzaje, których nie ma w manifeście

SDK oferuje też `Param::number` (skończona wartość `f64`) i `Param::secret` (klucz pola danych uwierzytelniających). Wersja manifestu 1 nie ma rodzaju dla żadnego z nich, więc agent odrzuca każdą wartość takiego parametru jako niezadeklarowaną. Działają pod atrapą hosta w testach jednostkowych, ale nie da się do nich dotrzeć przez agenta. Używaj `integer` albo `text`, a sloty danych uwierzytelniających wskazuj w kodzie; zobacz [Dane uwierzytelniające](../credentials/).

## Walidacja

Parametry są walidowane dwukrotnie, zanim uruchomi się handler.

1. **Agent** normalizuje parametry względem manifestu, niezależnie od tego, co uruchomiło akcję. Nieznane parametry, błędne typy, wartości spoza zakresu, opcje, których nie ma na liście, zbyt długie teksty i brakujące parametry wymagane kończą się błędem `invalid-parameters` z komunikatem wymieniającym każdy problem. Pusty wymagany tekst liczy się jako brakujący. Wartości domyślne są uzupełniane. Referencje i szablony rozwiązuje agent, więc wtyczka zawsze widzi wyłącznie wartości literalne.
2. **SDK** sprawdza to samo względem deklaracji `Param`: nieznane klucze, rodzaje, ograniczenia i opcje. Uzupełnia wartości domyślne i odrzuca z kodem `invalid-parameters`, podając nazwę parametru, ale nigdy nie cytując jego wartości.

Parametr, który deklaruje manifest, a nie deklaruje kod, SDK odrzuca jako nieznany. To jeden z powodów, żeby pilnować zgodności jednego z drugim za pomocą testu.

Reguły, których rodzaje nie potrafią wyrazić, musisz sprawdzić samodzielnie. Zwróć `invalid-parameters` albo własny kod:

```rust
fn is_service(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
```

## Odczytywanie wartości przez `Args`

`Args::get::<T>(key)` zwraca sprawdzoną wartość. `T` to `String`, `i64`, `f64`, `bool`, `SecretHandle` albo `Option<T>` jednego z nich.

```rust
    let settings = ctx.settings()?;
    let service = service_of(args, &settings)?;
    let min_uptime: i64 = args.get("minUptime")?; // default filled in: 99
    let notify: bool = args.get("notify")?;
```

Parametr opcjonalny bez wartości domyślnej odczytuj jako `Option<T>`:

```rust
/// The service from the parameter, else the `defaultService` setting.
fn service_of(args: &Args<'_>, settings: &Settings) -> Result<String, PluginError> {
    let service = match args.get::<Option<String>>("service")? {
        Some(s) if !s.trim().is_empty() => s.trim().to_owned(),
        _ => match settings.text("defaultService") {
            Some(s) if !s.trim().is_empty() => s.trim().to_owned(),
            _ => {
                return Err(PluginError::new(
                    "service-required",
                    "name a service or set a default service in the plugin's settings",
                ));
            }
        },
    };
    if !is_service(&service) {
        return Err(PluginError::new(
            "invalid-service",
            "a service name has lowercase letters, digits and hyphens only",
        ));
    }
    Ok(service)
}
```

Błędy zwracane przez `get` są typowane:

| Sytuacja | Kod |
| --- | --- |
| Parametr opcjonalny bez wartości, odczytany jako `T` zamiast `Option<T>` | `invalid-parameters` |
| Klucz, którego akcja nie deklaruje | `plugin-error` |
| Typ `T`, który nie pasuje do rodzaju parametru | `plugin-error` |

`Args::has(key)` mówi, czy wartość jest obecna, a `Args::values()` daje surowe sprawdzone wartości.

## Ustawienia wtyczki

Ustawienia to wartości, które użytkownik ustawia raz dla całej wtyczki, na przykład domyślne repozytorium. Są zaimplementowane w ABI 1.

Zadeklaruj je w `contributes.settings.fields`. Pole to parametr z jedną dodatkową właściwością, `portability`:

```json
"settings": {
  "fields": [
    {
      "key": "defaultService",
      "label": "Default service",
      "description": "Checked when an action names no service.",
      "required": false,
      "kind": { "type": "text", "maxLength": 64 },
      "portability": "portable"
    }
  ]
}
```

`portability` przyjmuje wartość `portable`, `machineSpecific` (domyślną) albo `sensitive`. Wtyczka ma co najwyżej 32 pola ustawień.

Odczytuj je przez `Context::settings()`, co nie wymaga żadnego uprawnienia. Zwrócony obiekt `Settings` ma metody `text(key)`, `integer(key)`, `boolean(key)`, `get::<T>(key)` i `contains(key)`.

Co warto wiedzieć o ustawieniach:

- Agent waliduje zapisane wartości tak jak parametry akcji i odrzuca wartość, która nie pasuje do swojego pola.
- Zapisana wartość, która po aktualizacji przestała pasować, jest zastępowana wartością domyślną. Pole, które nie ma ani wartości, ani wartości domyślnej, jest nieobecne.
- Brak wymaganego ustawienia jest zgłaszany aplikacji, ale akcja mimo to się uruchamia. Obsłuż brak wartości w kodzie, tak jak robi to `service_of`.
- W ABI 1 nie ma powiadomień o zmianie. Odczytuj ustawienia przy każdym wywołaniu.
- Ustawienia są przechowywane w bazie danych wtyczek agenta i nie są eksportowane razem z konfiguracją.
- Nie ma ustawień będących sekretami. Nigdy nie proś użytkownika o umieszczenie sekretu w ustawieniu albo w parametrze; użyj pola danych uwierzytelniających.

Ustawienie tekstowe może też przechowywać adres serwera podany przez użytkownika, jeśli uprawnienie `network` wskazuje je w `userOrigins`; zobacz [Uprawnienia](../permissions/#adresy-serwerów-podane-przez-użytkownika).

## Dokumentacja źródłowa

[docs/marketplace/sdk/actions.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/actions.md) w zakresie parametrów oraz [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md) w zakresie ustawień.
