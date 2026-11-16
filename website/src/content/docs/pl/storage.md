---
title: "Pamięć trwała"
description: "Trwały magazyn klucz-wartość wtyczki: API pamięci w Context, limity, izolacja oraz to, co przetrwa które zdarzenie."
group: capabilities
order: 10
---

Każda wtyczka ma własny trwały magazyn typu klucz-wartość. To miejsce na wszystko, co musi przetrwać dłużej niż jedno wywołanie: ostatni wynik, kursor, niewielką pamięć podręczną. Pamięć trwała nie wymaga uprawnienia, jest odizolowana dla każdej wtyczki i znajduje się w zaszyfrowanej bazie danych wtyczek agenta.

## API

Wszystkie wywołania pamięci trwałej są metodami `Context`. Wartości mają postać JSON.

| Metoda | Zwraca | Działanie |
| --- | --- | --- |
| `storage_get(key)` | `Option<serde_json::Value>` | Odczytuje wartość; `None`, gdy klucz nie jest ustawiony |
| `storage_get_as::<T>(key)` | `Option<T>` | Odczytuje wartość i deserializuje ją do typu `serde` |
| `storage_set(key, value)` | `()` | Zapisuje dowolną wartość, która serializuje się do JSON |
| `storage_delete(key)` | `()` | Usuwa klucz |
| `storage_keys(prefix)` | `Vec<String>` | Zwraca posortowaną listę kluczy wtyczki, opcjonalnie tylko tych zaczynających się od prefiksu |
| `storage_usage()` | `StorageUsage` | Podaje `used_bytes`, `keys`, `quota_bytes` i `max_keys` |

Każda z nich zwraca `Result` z błędem `CapabilityError`.

## Przechowywanie wartości typowanych

Zdefiniuj typ `serde` dla danych, które przechowujesz:

```rust
/// What `check` keeps in storage.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastCheck {
    pub service: String,
    pub status: String,
    pub checked_at: String,
}
```

Odczytaj poprzednią wartość, a potem zapisz nową:

```rust
    let previous: Option<LastCheck> = ctx.storage_get_as(LAST_CHECK)?;
    ctx.storage_set(LAST_CHECK, &last)?;
```

Późniejsza akcja może ją odczytać bez żadnego żądania sieciowego:

```rust
    let Some(last) = ctx.storage_get_as::<LastCheck>(LAST_CHECK)? else {
        return Ok(Output::skipped("nothing checked yet"));
    };
```

Jeśli w późniejszej wersji zmienisz kształt przechowywanego typu, wartości zapisane przez starą wersję nadal będą istnieć po aktualizacji. Nowe pola oznacz jako opcjonalne albo nadaj im wartości domyślne, aby stare wartości wciąż dawały się zdeserializować.

## Limity

| Limit | Wartość |
| --- | --- |
| Łączny rozmiar | Domyślnie 256 KiB |
| Liczba kluczy | 1000 |
| Długość klucza | Od 1 do 128 znaków, bez znaków sterujących |
| Jedna wartość | Najwyżej 64 KiB w postaci JSON |
| Częstość wywołań | 100 wywołań pamięci trwałej na sekundę |

Bajty liczy się jako klucz plus wartość JSON, w UTF-8.

### Podnoszenie limitu

Uprawnienie `storage` podnosi łączny rozmiar do wartości `quotaKb`, od 257 do 10 240 KiB (10 MiB):

```json
{
  "kind": "storage",
  "quotaKb": 1024,
  "justification": "Keeps a history of the last checks of each service you follow."
}
```

To uprawnienie jak każde inne: użytkownik je widzi i musi je zatwierdzić. Proś o nie tylko wtedy, gdy 256 KiB naprawdę nie wystarcza. Liczba kluczy pozostaje równa 1000.

### Po osiągnięciu limitu

Zapis, który powiększyłby magazyn ponad limit, kończy się błędem rodzaju `Failed` ze szczegółem zaczynającym się od `storage-quota`. Zapis, który nie powiększa magazynu, jest nadal dozwolony powyżej limitu, co ma znaczenie po cofnięciu uprawnienia `storage`: wtyczka może wtedy wciąż zmniejszać lub nadpisywać swoje dane.

`storage_usage()` pozwala akcji sprawdzić zajętość z wyprzedzeniem albo podać, ile pamięci wykorzystuje.

## Izolacja

- Magazyn należy do jednej wtyczki na jednym komputerze. Żadna wtyczka nie ma dostępu do kluczy innej wtyczki; identyfikator wtyczki nigdy nie jest częścią żądania, ponieważ agent wie, proces hosta której wtyczki je wysyła.
- Znajduje się w zaszyfrowanej bazie danych wtyczek agenta.
- Nigdy nie jest eksportowany razem z konfiguracją DeckPilota.
- Nie jest miejscem na sekrety. Token powinien trafić do [pola danych uwierzytelniających (slotu)](../credentials/), z którego kod wtyczki nie może go odczytać.

## Co przetrwa

| Zdarzenie | Pamięć trwała |
| --- | --- |
| Ponowne uruchomienie agenta | zachowana |
| Wyłączenie i ponowne włączenie wtyczki | zachowana |
| Aktualizacja wtyczki lub przywrócenie poprzedniej wersji | zachowana |
| Awaria procesu hosta wtyczki | zachowana |
| Cofnięcie uprawnienia | zachowana |
| Odinstalowanie wtyczki | **usunięta** |

Przy odinstalowaniu pamięć trwała jest zawsze usuwana. To jedyny trwały stan wtyczki, który jej kod może odczytać: wartości w zmiennych statycznych modułu żyją tylko tak długo, jak proces hosta, a opublikowane [zmienne](../variables/) są usuwane przy ponownym uruchomieniu agenta.

## Pamięć trwała a brak zegara

Moduł nie ma zegara, więc nie potrafi samodzielnie opatrzyć zapisywanych danych znacznikiem czasu. Przykłady używają nagłówka `Date` odpowiedzi HTTP jako czasu sprawdzenia:

```rust
        // The guest has no clock: the time of the check is the answer's Date header.
        checked_at: response.header("date").unwrap_or_default().chars().take(64).collect(),
```

Z tego samego powodu wtyczka nie może unieważniać wartości w pamięci podręcznej na podstawie ich wieku, o ile nie ma takiego czasu odniesienia.

## Testowanie

`MockHost::allow_all()` odpowiada na każdy odczyt z pamięci trwałej wartością `null`, którą odczytuje się jako „nie ustawiono”. Host ze skryptowanymi odpowiedziami może zwracać zapisane wartości, a `host.requests()` pokazuje, co zostało zapisane:

```rust
    assert!(host.requests().iter().any(|r| matches!(r,
        CapabilityRequest::StorageSet(s) if s.key == "last.check")));
```

Przykład Developer Tools idzie dalej i trzyma w swojej atrapie hosta mapę w pamięci, dzięki czemu wartość zapisaną przez jedno wywołanie odczytuje następne. Zobacz [Testowanie](../testing/).

## Dokumentacja źródłowa

Sekcja Storage w [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md).
