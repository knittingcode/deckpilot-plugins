# Memory (`deckpilot.memory`)

Gra w pary na Twoich klawiszach: każdy klawisz to karta, zakryta. Odkryj
dwie; jeśli pasują, zostają odkryte, jeśli nie, zakrywają się przy
następnym ruchu. Znajdź wszystkie pary w jak najmniejszej liczbie ruchów.
Każda gra jest tasowana od nowa, a wtyczka pamięta Twoją najlepszą grę dla
każdego rozmiaru planszy.

English: [README.md](README.md).

> **Wymaga DeckPilota z plugin API 1.2** (stan klawiszy). Starsze wersje
> nie proponują instalacji wtyczki.
>
> **Na razie dwa gesty na kartę.** Klawisz pokazuje stan wtyczki tylko dla
> akcji, która niczego nie zmienia, a odkrycie karty zmienia planszę.
> Dlatego karta to dwie akcje na jednym klawiszu: **kliknięcie** pokazuje
> kartę (`Karta`), **długie naciśnięcie** ją odkrywa (`Odkryj kartę`). Gra
> jednym naciśnięciem na kartę czeka na zmianę w DeckPilocie, która
> pozwoli pokazać kartę także akcji, która ją odkrywa
> ([knittingcode/deckpilot#306](https://github.com/knittingcode/deckpilot/pull/306),
> w przeglądzie). Gdy wejdzie, następna wersja tej wtyczki przejdzie na
> jedno naciśnięcie na kartę ([Czego nie robi](#czego-nie-robi)).

## Konfiguracja

1. Zainstaluj i włącz wtyczkę. Zezwól na jej jedno uprawnienie:
   pokazywanie stanu na jej klawiszach.
2. W ustawieniach wtyczki wybierz liczbę **Par** (domyślnie 6, czyli 12
   kart; od 2 do 16, czyli od 4 do 32 kart). Dotyczy następnej nowej gry.
3. Umieść **Nowa gra** na jednym klawiszu i opcjonalnie **Wynik** na
   innym.
4. Dla każdej karty, numerowanej od 1 do liczby kart, ustaw jeden klawisz:
   - **Kliknięcie**: `Karta`, z parametrem **Karta** ustawionym na numer
     karty;
   - **Długie naciśnięcie**: `Odkryj kartę`, z tym samym numerem.

   Nie nadawaj klawiszowi tytułu albo nadaj krótki, na przykład jego
   numer: w trakcie gry zastępuje go awers karty.
5. Naciśnij **Nowa gra**.

Karty o numerze większym niż rozmiar planszy są pomijane (ich klawisz
pokazuje Twój tytuł), więc deck z 15 klawiszami zmieści planszę 6 par i
zostawi trzy klawisze na Nową grę, Wynik i cokolwiek innego.

## Jak grać

- **Długo naciśnij** kartę, aby ją odkryć. Odkryj drugą:
  - jeśli pasują, obie zostają odkryte ze znacznikiem;
  - jeśli nie, obie zostają odkryte, abyś mógł je zapamiętać, i zakrywają
    się, gdy odkryjesz następną kartę.
- **Kliknięcie** karty tylko mówi, co pokazuje (w komunikatach stanu
  DeckPilota); nigdy nie zmienia gry.
- Jeden ruch to dwie karty. Po znalezieniu ostatniej pary zegar staje, a
  klawisz Wyniku pokazuje ruchy i czas, z gwiazdką, jeśli to Twoja
  najlepsza gra tego rozmiaru.
- **Nowa gra** porzuca trwającą grę.

## Co pokazują klawisze

| Klawisz | Pokazuje |
| --- | --- |
| Karta zakryta | `?` na niebieskim |
| Karta odkryta | swój obrazek (gwiazdę, serce, dzwonek…) i jego nazwę, na własnym kolorze |
| Karta ze znalezionej pary | to samo, ze znacznikiem i paskiem u góry |
| Wynik w trakcie gry | znalezione pary, np. `3/6`, ruchy i pasek postępu |
| Wynik po wygranej | ruchy i czas, na zielonym, z gwiazdką za rekord |
| Wynik bez gry | `Memory` i najmniejszą liczbę ruchów najlepszej gry jako plakietkę |

Szesnaście obrazków, po jednym na parę, rysuje sam DeckPilot w
rozdzielczości Twojego urządzenia; wtyczka tylko je nazywa. Nazwy
obrazków na klawiszach są po angielsku (Star, Heart…). Urządzenie lub
element bez wyświetlacza nic nie pokazuje, a gra nadal działa przez
komunikaty stanu.

## Akcje

| Akcja | Id | Co robi | Skutek |
| --- | --- | --- | --- |
| Nowa gra | `new-game` | Tasuje planszę o rozmiarze z ustawień i uruchamia zegar | zmienia |
| Odkryj kartę | `flip` | Odkrywa jedną kartę (parametr **Karta**, od 1 do 32) | zmienia |
| Karta | `card` | Pokazuje jedną kartę na jej klawiszu; uruchomienie niczego nie zmienia | tylko odczyt |
| Wynik | `score` | Pokazuje wynik na swoim klawiszu; uruchomienie go podaje | tylko odczyt |

Wyjścia dla workflow: Nowa gra daje `pairs` i `cells`; Odkryj kartę daje
`result` (`first`, `match`, `miss` lub `won`), `moves` i `pairsFound`;
Wynik daje `moves`, `pairsFound` i `pairs`. Odkrycie karty, która już jest
odkryta, jest pomijane, a nie jest błędem.

Zmienne: `{{ plugins.deckpilot.memory.moves }}` i
`{{ plugins.deckpilot.memory.pairsFound }}`, dla bieżącej gry.

## Ustawienia

| Ustawienie | Domyślnie | Znaczenie |
| --- | --- | --- |
| Pary | 6 | Ile par ma nowa gra, od 2 do 16 (od 4 do 32 kart) |

## Uprawnienia i dane

- **Pokazywanie stanu na jej klawiszach** (`device`, `state`): aby pokazać
  każdą kartę i wynik. Wtyczka opisuje klawisz (tekst, obrazek z zestawu
  DeckPilota, kolor, znacznik); rysuje go DeckPilot. Wtyczka nigdy nie
  wie, jakiego urządzenia ani klawisza używasz.
- Tasowanie bierze 8 losowych bajtów ze źródła losowości DeckPilota na
  każdą nową grę, a zegar jest odczytywany na początku i przy każdym
  ruchu. Żadne z nich nie wymaga uprawnienia.
- Plansza i Twoje najlepsze gry są przechowywane we własnym magazynie
  wtyczki na tym komputerze i usuwane przy jej odinstalowaniu. Nic nie
  jest nigdzie wysyłane: wtyczka nie ma dostępu do sieci i nie pokazuje
  powiadomień.

## Błędy

| Kod | Kiedy |
| --- | --- |
| `no-game` | Żadna gra nie trwa albo zapisanej nie dało się odczytać: naciśnij Nowa gra |
| `no-such-card` | Numeru karty nie ma na tej planszy: przypisz karty od 1 do liczby kart |
| `game-over` | Wszystkie pary zostały znalezione: naciśnij Nowa gra |

## Czego nie robi

- **Jednego naciśnięcia na kartę.** Odkrycie karty zmienia planszę, a
  DeckPilot pokazuje stan wtyczki tylko na klawiszach akcji, które niczego
  nie zmieniają. Stąd kliknięcie, aby pokazać, i długie naciśnięcie, aby
  odkryć. Zmiana, która pozwala na stan przy każdej akcji (bo pokazywanie
  stanu nigdy niczego nie zmienia), czeka na przegląd w DeckPilocie
  ([#306](https://github.com/knittingcode/deckpilot/pull/306)). Wtyczka
  nie oznacza akcji odkrywania jako tylko do odczytu, aby to obejść: to
  mówiłoby o niej coś nieprawdziwego.
- **Zakrywania nietrafionej pary po chwili.** Wtyczka nie ma timera;
  nietrafiona para zostaje odkryta do Twojego następnego ruchu.
- **Kilku graczy**, poziomów ani limitu czasu.

## Budowanie i testy

W katalogu `first-party/`:

```sh
cargo test --locked -p deckpilot-memory
cargo build --locked -p deckpilot-memory --target wasm32-unknown-unknown --profile guest
```

`tools/first-party.sh` sprawdza układ i polskie teksty oraz pakuje wtyczkę
pod testowym wydawcą (wtyczki first-party instalują się dziś tylko z
pakietu aplikacji). Testy obejmują grę wyczerpująco (równomierność
tasowania, każdy rodzaj ruchu, uszkodzone plansze), każdą akcję wobec
symulowanego agenta, stany klawiszy i każdy kod błędu.
