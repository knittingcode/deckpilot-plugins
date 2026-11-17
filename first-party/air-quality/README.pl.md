# Jakość powietrza (`deckpilot.air-quality`)

PM2.5, PM10 i polski indeks jakości powietrza jednej stacji pomiarowej, z
czasem pomiaru: jako komunikat, jako wyjścia dla przepływów pracy i jako
zmienne. Dane pochodzą z [GIOŚ](https://powietrze.gios.gov.pl/pjp/home)
(Główny Inspektorat Ochrony Środowiska), bez konta i bez klucza.

Źródło danych: GIOŚ - EKOINFONET
([CC BY 4.0](https://creativecommons.org/licenses/by/4.0/deed.pl)). Wtyczka
pokazuje fragment tych danych (najnowsze wartości jednogodzinne i indeks
jednej stacji), zaokrągla stężenia do pełnych µg/m³, a w języku angielskim
tłumaczy nazwy kategorii indeksu.

English: [README.md](README.md).

> **Status.** Wersja 0.1.0 jest budowana, testowana i pakowana przez CI i
> **nie jest opublikowana**. Nie da się jej jeszcze zainstalować z rejestru
> ([first-party/README.md](../README.md)). Przed publikacją trzeba ponownie
> przeczytać warunki dostawcy ([niżej](#warunki-dostawców)).

## Na klawiszu

*Od wersji 0.2.0; wymaga DeckPilota z API wtyczek 1.2.* Klawisz przypisany
do akcji **Aktualna jakość powietrza** pokazuje kategorię indeksu, na
przykład `Dobry`, na jej kolorze, a pod nią wartości PM (`PM2.5 12 PM10
20`, w µg/m³). Kolory odpowiadają skali indeksu tak dokładnie, jak
pozwala paleta DeckPilota: morski dla bardzo dobrego, zielony dla
dobrego, bursztynowy dla umiarkowanego i dostatecznego, czerwony dla
złego, fioletowy dla bardzo złego. Odczyt bez indeksu pokazuje `Brak
indeksu` bez koloru.

Klawisz pokazuje **ostatni odczyt pobrany przez uruchomienie akcji**
(naciśnięcie klawisza albo przepływ z harmonogramem). Samo pokazywanie
nigdy nie pyta dostawcy. Stosuje regułę [aktualności](#aktualność)
wtyczki do chwili obecnej: jeśli odczyt nie został pobrany w ciągu 30
minut **i** zmierzony w ciągu 3 godzin, wartości zostają, ale są
oznaczone znacznikiem ostrzeżenia i wiekiem pomiaru jako plakietką (`4h`,
`2d`), albo `?`, gdy jest nieznany. Nigdy nie są pokazywane jako aktualne.

Przed pierwszym odczytem albo bez wybranej stacji klawisz pokazuje
nadany przez Ciebie tytuł. Teksty na klawiszach wyglądają tak, jak są
(`Zły`), w DeckPilocie, który rysuje litery z diakrytykami (od DeckPilot
#307; ma go każde wydanie z API wtyczek 1.2). Urządzenie bez wyświetlaczy
nic nie pokazuje.

## Czego wtyczka nie robi

API wtyczek 1.2 pozwala wtyczce opisać, co pokazuje klawisz (wyżej), ale
nie uruchamiać się samodzielnie ani mieć timera. Dlatego ta wersja **nie**:

- **odświeża się sama.** Działa, gdy naciśniesz klawisz albo gdy uruchomi
  ją przepływ pracy. Aby zmienne były aktualne, użyj przepływu z
  harmonogramem ([niżej](#odświeżanie-według-harmonogramu)).
- **zna czas przy naciśnięciu klawisza**, więc nie wie, jak stara jest jej
  pamięć podręczna, zanim zapyta ([niżej](#aktualność)).
- **korzysta z Airly** ani z żadnego dostawcy poza GIOŚ
  ([niżej](#airly-nie-w-tej-wersji)). Nie ma klucza do wpisania ani
  miejsca na poświadczenia.
- znajduje najbliższej stacji (nigdy nie pyta o lokalizację), nie obejmuje
  miejsc poza Polską, nie pokazuje prognozy, historii, mapy ani alertów i
  nie wysyła powiadomień.

## Akcje

| Akcja | Co robi |
| --- | --- |
| `current` (Aktualna jakość powietrza) | Indeks, PM2.5 i PM10 wybranej stacji, indeks NO2, O3 i SO2, jeśli stacja je ma, oraz czas wartości |
| `find-station` (Znajdź stację) | Wyszukuje stacje według `query` (miasto, nazwa stacji, województwo) i wypisuje do ośmiu z ich numerami |
| `select-station` (Wybierz stację) | Zapisuje stację o numerze `station` jako tę, dla której działa `current` |
| `forget-station` (Zapomnij stację) | Usuwa wybraną stację i jej zapisane wartości oraz czyści zmienne |

`current` przyjmuje opcjonalne `at`, czas uruchomienia
([niżej](#odświeżanie-według-harmonogramu)).

### Wyjścia akcji `current`

| Wyjście | Rodzaj | Przykład |
| --- | --- | --- |
| `summary` | tekst | `Warszawa, Al. Niepodległości (mazowieckie): jakość powietrza: Dobry (indeks 1; Polski indeks jakości powietrza, od 0 (najlepszy) do 5). PM2.5 15 µg/m³, PM10 23 µg/m³. Indeks zanieczyszczeń: NO2: Dobry. Pomiar: 2026-11-16 19:00 (czas polski). Źródło danych: GIOŚ - EKOINFONET.` Zaczyna się od `[nieaktualne]`, gdy nie wiadomo, czy dane są aktualne |
| `index` | liczba całkowita | `1`; brak, gdy stacja nie ma teraz indeksu |
| `category` | tekst | `Dobry` (w języku z ustawień); `Brak indeksu` |
| `categoryName` | tekst | `good` (stała nazwa, [niżej](#indeks)); `none` |
| `pm25`, `pm10` | liczba całkowita | `15`, `23` (µg/m³, zaokrąglone); brak, gdy stacja nie ma takiej wartości |
| `unit` | tekst | `µg/m³` |
| `pm25Time`, `pm10Time`, `indexTime` | tekst | `2026-11-16 19:00`, czas polski; każde nieobecne razem ze swoją wartością |
| `no2Category`, `o3Category`, `so2Category` | tekst | `good`; brak, gdy stacja nie ma indeksu danego zanieczyszczenia |
| `station` | tekst | `Warszawa, Al. Niepodległości (mazowieckie)` |
| `provider` | tekst | `gios` |
| `measuredAt` | tekst | `2026-11-16 19:00`: **najstarszy** z powyższych czasów, czas polski |
| `measurementAgeMinutes` | liczba całkowita | `42`: od `measuredAt` do teraz; brak, gdy wtyczka nie zna bieżącego czasu |
| `stale` | wartość logiczna | `true`, chyba że wtyczka potrafi wykazać, że wartości są świeże ([niżej](#aktualność)) |
| `source` | tekst | `network` albo `cache` |
| `fetchedAt` | tekst | `2026-11-16T18:42:09Z`; brak, gdy nieznany |
| `ageMinutes` | liczba całkowita | minuty od pobrania wartości; brak, gdy nieznane |
| `problem` | tekst | kod błędu nieudanego żądania, gdy zamiast nowych pokazano zapisane dane |
| `attribution` | tekst | `Źródło danych: GIOŚ - EKOINFONET (CC BY 4.0; fragment danych, wartości zaokrąglone).` |

Każdy komunikat kończy się informacją o źródle, a `summary` jej pierwszym
zdaniem. Jeśli pokazujesz wartość z tej wtyczki gdzie indziej (w
powiadomieniu, pliku, innej usłudze), pokaż razem z nią źródło i czas
pomiaru: po to są `attribution` i `measuredAt`.

### Wyjścia akcji `find-station` i `select-station`

`find-station`: `count` (wszystkie wyniki), `shown` (ile wypisano, najwyżej
osiem), `matches` (wiersz na stację: `530: Warszawa, Al. Niepodległości
(mazowieckie)`), `firstId` (numer pierwszego wyniku), `source` (`network`
albo `cache`: lista stacji) i `attribution`.

`select-station`: `selected` (stacja), `stationId`, `hasPm25`, `hasPm10`
(czy stacja je mierzy) i `attribution`.

Współrzędne i adresy nigdy nie są wyjściem: wtyczka ich nie odczytuje.

## Zmienne

`current` publikuje `pm25`, `pm10`, `index` (liczby całkowite), `category`,
`categoryName`, `summary`, `measuredAt`, `station` (teksty) i `stale`
(wartość logiczna). Wartość, której stacja teraz nie ma, jest czyszczona, a
nie pozostawiana z poprzedniego uruchomienia.

W przepływach pracy odczytasz je jako
`{{ plugins.deckpilot.airQuality.pm25 }}`. Gdy akcja się nie powiedzie,
`stale` przyjmuje wartość prawda, a pozostałe zachowują ostatnie wartości.
Zmienne znikają po ponownym uruchomieniu DeckPilot, do następnego
uruchomienia akcji. `select-station` i `forget-station` czyszczą wszystkie.

## Wybór stacji

1. Uruchom *Znajdź stację* z nazwą miasta (strona wtyczki w DeckPilot ma
   przycisk Uruchom): `Kraków` albo `krakow`; polskie znaki można pominąć.
   Kolejne słowa zawężają wyszukiwanie (`warszawa wokalna`); działa też
   nazwa województwa. Akcja wypisuje do ośmiu stacji, każdą z numerem na
   początku.
2. Uruchom *Wybierz stację* z tym numerem. Wtyczka pyta GIOŚ, które z
   PM2.5 i PM10 stacja mierzy, i zapisuje wybór na tym komputerze.

GIOŚ ma około 290 stacji w ponad 200 miejscowościach. Wiele mierzy tylko
niektóre zanieczyszczenia: stację bez PM2.5 i PM10 można wybrać, a
*Aktualna jakość powietrza* poda wtedy tylko jej indeks. Jeśli stanowiska
stacji się zmienią, wybierz ją ponownie.

Ustawienia: *Język tekstów* (`en` albo `pl`) dla kategorii i komunikatów
oraz *Dostawca danych*, który ma jedną wartość, `gios`. Komunikaty błędów
są po angielsku. Nazwy i opisy w aplikacji są angielskie, bo manifest
wtyczki nie ma jeszcze tłumaczeń; polskie znajdują się w
[locales/pl.json](locales/pl.json).

## Odświeżanie według harmonogramu

Aby zmienne były aktualne, dodaj przepływ pracy z **wyzwalaczem
harmonogramu**, który uruchamia *Aktualną jakość powietrza* co 30 albo 60
minut: dane są godzinowe, a GIOŚ prosi o pobieranie nie częściej niż dwa
razy na godzinę. Przekaż czas wyzwolenia (pole zdarzenia `firedAt`,
`RRRR-MM-DD GG:MM`) do parametru *Czas uruchomienia* (`at`) akcji przez
wejście przepływu. Ten czas to jedyny zegar, jaki wtyczka może mieć, zanim
zapyta sieć, i dzięki niemu pamięć podręczna działa zgodnie z zamierzeniem:

- odczyt pobrany mniej niż 30 minut temu jest używany ponownie bez
  żądania;
- gdy GIOŚ odpowie „za dużo żądań” (HTTP 429), żadne żądanie nie jest
  wysyłane, dopóki nie minie czas, o który poprosił (15 minut, jeśli go nie
  podał).

Uruchomienie bez `at`, na przykład naciśnięcie klawisza, nie wie, jak stary
jest zapisany odczyt, i za każdym razem pyta GIOŚ (trzy małe żądania).
**Wtyczka nie potrafi wymusić dwóch pobrań na godzinę przy naciśnięciach
klawisza**; to ograniczenie API wtyczek 1.0 (brak zegara) i do czasu, aż go
dostanie, pilnujesz tego Ty. Najprościej: niech pyta przepływ z
harmonogramem, a Ty odczytuj zmienne.

## Aktualność

Jakość powietrza sprzed kilku godzin nie jest jakością powietrza teraz,
więc wtyczka nigdy nie sugeruje, że wartość jest bieżąca. Podaje czas,
który GIOŚ przypisuje każdej wartości, a `stale` ma wartość **prawda,
chyba że spełnione są oba** warunki:

- odczyt został pobrany w tym uruchomieniu albo mniej niż 30 minut temu
  według czasu, który wtyczka zna;
- **najstarsza** wartość w nim została zmierzona najwyżej 3 godziny przed
  chwilą obecną, według chwili obecnej, którą wtyczka zna.

GIOŚ publikuje wartość z danej godziny w ciągu następnej godziny (16
listopada 2026 r. wartości z 19:00 były dostępne o 19:42), więc działająca
stacja mieści się w dwóch godzinach; trzy dopuszczają jedną pominiętą
publikację. Stacja z niedziałającym analizatorem dalej podaje ostatnie
wartości: przychodzą one ze `stale` równym prawda i komunikatem
zaczynającym się od „Nieaktualne”.

Wtyczka nie ma zegara. Jej „chwilą obecną” jest nagłówek `Date` odpowiedzi
GIOŚ w tym uruchomieniu albo czas pobrania zapisanego odczytu powiększony o
minuty między dwiema wartościami `at`. Gdy żadna nie jest znana (nie udało
się połączyć z GIOŚ i nie ma `at`), zapisane wartości są pokazywane ze
`stale` równym prawda, z czasem pobrania i bez wieku; komunikat mówi, że
wtyczka nie wie, jak są stare.

Czasy w wyjściach i komunikatach to czas polski, tak jak podaje je GIOŚ
(`fetchedAt` jest w UTC). Aby porównać je z chwilą obecną, wtyczka
przelicza je według europejskiej reguły czasu letniego.

Gdy żądanie się nie powiedzie i nic nie jest zapisane, akcja kończy się
błędem tego żądania. Odmowa uprawnienia sieciowego jest zawsze błędem:
zapisane dane nie są wtedy pokazywane.

## Indeks

Polski indeks jakości powietrza GIOŚ ma sześć kategorii. `index` to liczba
GIOŚ, `categoryName` stała nazwa, a `category` tekst w języku z ustawień.

| `index` | `categoryName` | Po angielsku | Po polsku (nazwa GIOŚ) |
| --- | --- | --- | --- |
| 0 | `very-good` | Very good | Bardzo dobry |
| 1 | `good` | Good | Dobry |
| 2 | `moderate` | Moderate | Umiarkowany |
| 3 | `sufficient` | Sufficient | Dostateczny |
| 4 | `bad` | Bad | Zły |
| 5 | `very-bad` | Very bad | Bardzo zły |
| brak | `none` | No index | Brak indeksu |

Nazwy angielskie są tłumaczeniami tej wtyczki, nie GIOŚ. Indeks oblicza
GIOŚ z zanieczyszczeń mierzonych przez stację; wtyczka nie wylicza go z
wartości PM.

## Uprawnienia

| Uprawnienie | Po co |
| --- | --- |
| `network`: `api.gios.gov.pl`, tylko `GET` | Lista stacji, stanowiska wybieranej stacji oraz jej indeks i wartości PM |
| `device`: `state` | Indeks i wartości PM na klawiszach przypisanych do akcji Aktualna jakość powietrza ([Na klawiszu](#na-klawiszu)) |

Nic więcej: żadnych poświadczeń, powiadomień ani otwierania stron. Pamięć
wtyczki (najwyżej kilkadziesiąt kilobajtów) nie wymaga uprawnienia.

## Prywatność

- Wybrana stacja mówi z grubsza, gdzie jesteś. Jej numer trafia do GIOŚ w
  żądaniach o jej wartości (tak jak przy korzystaniu ze strony GIOŚ), a
  stacja jest przechowywana w pamięci wtyczki na tym komputerze, w
  komunikacie i wyjściach akcji oraz w zmiennej `station`. Nigdy nie jest
  zapisywana w dzienniku.
- To, co wpisujesz w *Znajdź stację*, nie jest nigdzie wysyłane: wtyczka
  raz pobiera całą publiczną listę i przeszukuje ją na Twoim komputerze.
- Wtyczka nie odczytuje lokalizacji ani współrzędnych. Współrzędne i adresy
  z listy stacji GIOŚ są odrzucane przy jej odczycie i nigdy nie są
  zapisywane.
- Żądania nie zawierają klucza, ciasteczek ani identyfikatora; nagłówek
  `User-Agent` to `DeckPilot-AirQuality/0.1.0`. GIOŚ widzi Twój adres IP,
  jak każda witryna.
- *Zapomnij stację* usuwa wybór i zapisane wartości; odinstalowanie wtyczki
  usuwa wszystko, co zapisała.

## Warunki dostawców

Przeczytane 16 listopada 2026 r. Przed publikacją wtyczki trzeba
przeczytać je ponownie.

### GIOŚ

Źródła: [strona API](https://powietrze.gios.gov.pl/pjp/content/api),
[regulamin portalu](https://powietrze.gios.gov.pl/pjp/content/terms_of_service)
(przeczytany w całości) i opis OpenAPI pod adresem
`https://api.gios.gov.pl/pjp-api/v3/api-docs`.

| Co mówi regulamin | Co robi wtyczka |
| --- | --- |
| Dane można ponownie wykorzystywać, w całości lub w części, bezpłatnie, na licencji CC BY 4.0 | Korzysta z nich bez klucza; podaje licencję w `attribution` |
| Źródło należy wskazać „w sposób czytelny i wyraźny”, w postaci „Źródło danych: GIOŚ - EKOINFONET” | Tymi słowami kończy się każdy komunikat i `summary`, zaczyna się `attribution`; są też w opisie w manifeście |
| Należy określić czas wytworzenia informacji | Każdy komunikat i `summary` zawiera czas pomiaru; wyjścia `measuredAt`, `pm25Time`, `pm10Time`, `indexTime` |
| Należy wskazać, czy informacje są wykorzystywane w całości, czy we fragmentach, i w jakich | `attribution` mówi, że to fragment; ten plik mówi, jaki |
| Przetworzenie (zmianę) informacji należy zaznaczyć | `attribution` informuje o zaokrągleniu, a po angielsku także o przetłumaczonych nazwach kategorii |
| „Udostępniane dane nie są na bieżąco weryfikowane, zatem mogą ulec zmianie”; wartości są w µg/m³; zdarzają się przerwy | Wartości są podawane tak, jak przyszły; o brakującej wartości wtyczka informuje |
| Użytkownik API „zobowiązuje się do pobierania danych nie częściej niż dwa razy na godzinę” | 30-minutowa pamięć podręczna, której uruchomienia z harmonogramu nie obejdą; **naciśnięcia klawisza mogą** ([wyżej](#odświeżanie-według-harmonogramu)) |
| Limity: 1500 żądań na minutę dla bieżących pomiarów i indeksu; „2 i 1500” dla listy stacji i stanowisk; 2 na minutę dla metadanych, archiwów i statystyk | Lista stacji to jedno żądanie (dwa, gdyby GIOŚ miał ponad 500 stacji), przechowywana tydzień; o stanowiska stacji wtyczka pyta raz, przy wyborze; odczyt to trzy żądania |
| Wersje usług sprzed `v1` wycofano 30 czerwca 2025 r. | Używa wyłącznie `/pjp-api/v1/rest/` |

Logo nie jest wymagane i żadne nie jest dołączone. Ikona to własny rysunek
tej wtyczki.

Zmierzone 16 listopada 2026 r.: lista ma 288 stacji; domyślna strona ma
20, a `size=500` zwraca wszystkie w jednej odpowiedzi o rozmiarze 87 KiB,
poniżej 512 KiB, które DeckPilot pozwala wtyczce odebrać. Zapisana bez
współrzędnych i adresów ma około 18 KiB, w częściach po 50 stacji, z
których każda jest daleko poniżej 64 KiB jednej wartości w pamięci.

### Airly: nie w tej wersji

Airly miało być drugim dostawcą, z własnym kluczem API użytkownika w
miejscu na poświadczenia. Jego [regulamin API](https://airly.org/en/legal/terms-of-service-airly-api/)
(obowiązujący od 1 stycznia 2021 r., przeczytany w całości, po angielsku)
nie pozwala na to jednoznacznie, więc wtyczka nie ma kodu, hosta ani
poświadczeń Airly:

- § 7 ust. 5 lit. d: klientowi planu bezpłatnego nie wolno używać „API,
  danych ani Bazy danych uzyskanych przez API na jakichkolwiek stronach
  internetowych, platformach ani w aplikacjach mobilnych (komercyjnych lub
  niekomercyjnych)”. Czy aplikacja na komputer, która pokazuje właścicielowi
  klucza jego własne dane, jest „platformą”, może rozstrzygnąć Airly, nie
  my.
- § 9 ust. 1 lit. e: klient musi „umieścić odnośnik do Platformy i logo
  Airly w pobliżu wykorzystywanych danych”. Wynikiem wtyczki jest tekst;
  API wtyczek 1.0 nie potrafi pokazać przy nim obrazu. § 10 udziela licencji
  na logo wyłącznie klientowi, bez modyfikacji, więc wtyczka nie mogłaby go
  też dołączyć.
- § 9 ust. 1 wymaga także wskazania Airly jako źródła, dokładnego czasu
  pomiaru, informacji o każdej modyfikacji i użytej skali; to wtyczka
  mogłaby spełnić.
- § 6: klucza nie wolno udostępniać (miejsce na poświadczenia to
  zapewnia), jedno konto i jeden klucz na klienta, nie wolno przekraczać
  dziennej liczby żądań z planu (informują o niej nagłówki
  `x-ratelimit-remaining-day` i `-minute`). Strona pomocy Airly podaje 100
  żądań dziennie bez własnego czujnika. Dokumentacji dla programistów
  (`developer.airly.org`) nie udało się wczytać programowo, a odnośnik
  `termsOfService` w opisie samego API (`https://airly.eu/API-TOS.pdf`)
  nie działa.

Dodanie Airly wymaga pisemnej odpowiedzi Airly w dwóch pierwszych sprawach
oraz sposobu na pokazanie logo. Kod jest przygotowany na drugiego dostawcę
(`src/provider.rs`).

## Błędy

| Kod | Kiedy |
| --- | --- |
| `no-station` | Nie wybrano stacji |
| `station-not-found` | Wyszukiwanie nie znalazło stacji albo żadna stacja nie ma podanego numeru |
| `no-data` | Stacja nie podaje teraz ani PM2.5, ani PM10, ani indeksu |
| `provider-error` | GIOŚ odpowiedział błędem |
| `invalid-response` | Odpowiedzi GIOŚ nie udało się odczytać |
| `response-too-large` | Odpowiedź GIOŚ była większa, niż wtyczka przyjmuje |
| `timed-out` | GIOŚ nie odpowiedział na czas |
| `throttled` (wspólny) | GIOŚ odpowiedział HTTP 429 albo uruchomienie z harmonogramu nastąpiło przed upływem czasu, o który poprosił |
| `not-permitted` (wspólny) | Uprawnienie sieciowe nie zostało przyznane |
| `invalid-parameters` (wspólny) | Zapytanie krótsze niż dwie litery, numer stacji spoza zakresu, `at` w formacie innym niż `RRRR-MM-DD GG:MM` |
| `connection-failed`, `dns-failed`, `tls-failed`, … (wspólne) | Żądanie nie dotarło do GIOŚ |

W akcji `current` błędy dostawcy są zgłaszane tylko wtedy, gdy nie ma
zapisanego odczytu stacji do pokazania (oznaczonego jako `stale`, chyba że
nadal mieści się w swoich 30 minutach).

## Rozwój

```sh
cd first-party
cargo test --locked -p deckpilot-air-quality
cargo build --locked -p deckpilot-air-quality --target wasm32-unknown-unknown --profile guest
```

Testy jednostkowe zastępują agenta obiektem `MockHost` z SDK i odpowiadają
przygotowanymi odpowiedziami GIOŚ; nigdy nie łączą się z siecią.
`src/provider.rs` to cecha, którą implementuje kolejny dostawca;
`src/gios.rs` to GIOŚ; `src/cache.rs` to pamięć i jej reguły; `src/time.rs`
to arytmetyka kalendarzowa.
