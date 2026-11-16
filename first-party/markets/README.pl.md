# Rynki (`deckpilot.markets`)

Kursy walut dla DeckPilota: **referencyjne kursy wymiany euro Europejskiego
Banku Centralnego**, pobierane przez [Frankfurter](https://frankfurter.dev/)
bez konta i bez klucza. Przeliczenie, zmiana od poprzedniego dnia roboczego
albo w ciągu do 20 dni roboczych, zakres ostatnich kursów oraz alert, który
raz powiadamia, gdy kurs przekroczy Twój próg: jako komunikat, wyjścia dla
przepływów pracy i zmienne.

Źródło: referencyjne kursy wymiany euro Europejskiego Banku Centralnego,
przez Frankfurter. EBC publikuje je „wyłącznie w celach informacyjnych”.

**To dzienne kursy referencyjne, nie bieżące ceny.** EBC publikuje jeden
kurs każdej waluty raz na dzień roboczy, ok. 16:00 CET. Każde wyjście podaje,
którego dnia dotyczy kurs (`rateDate`), skąd pochodzi (`source`,
`attribution`), oraz ma `delayed` (zawsze `true`) i `stale`.

**Informacja, nie porada finansowa.** Wtyczka podaje opublikowane kursy
referencyjne. Niczego nie rekomenduje, a jej kursy nie są kursami, po
których można kupić lub sprzedać; EBC „zdecydowanie odradza” używanie ich do
transakcji.

English: [README.md](README.md).

> **Stan.** Wersja 0.1.0 jest budowana, testowana i pakowana przez CI i
> **nie jest opublikowana**. Nie da się jej jeszcze zainstalować z rejestru
> ([first-party/README.md](../README.md)).

## Czego nie robi

- **Żadnych bieżących cen ani notowań w ciągu dnia.** Jedynym źródłem są
  kursy referencyjne EBC. Przed ok. 16:00 CET, w weekendy i święta TARGET
  najnowszy kurs pochodzi z poprzedniego dnia roboczego; komunikat to mówi.
- **Bez cen kryptowalut** w tej wersji ([niżej](#kryptowaluty-odłożone)).
- **Bez notowań akcji** ([niżej](#notowania-akcji-niedostępne)).
- **Bez wykresu i niczego na klawiszu.** API wtyczek 1.0 nie pozwala
  rysować na klawiszu ani ustawiać jego tytułu. Wtyczka przechowuje krótką
  historię kursów każdej pary (najwyżej 32) i podaje minimum, maksimum oraz
  trend; sam wykres czeka na możliwość stanu klawisza proponowaną dla
  DeckPilota ([docs/plugin-factory.md](../../docs/plugin-factory.md) §6.1).
  Kod, który by go dostarczał, jest gotowy (`history::Snapshot::key`) i nie
  trafia na klawisz.
- **Bez samodzielnego odświeżania** (brak timerów) i **bez zegara**
  ([niżej](#pamięć-podręczna-i-czas)). Odświeża ją przepływ według
  harmonogramu.

## Akcje

| Akcja | Co robi |
| --- | --- |
| `rate` (Kurs wymiany) | Najnowszy kurs referencyjny `base`/`quote`, opcjonalne przeliczenie `amount`, zmiana od poprzedniego kursu, najniższy i najwyższy zapisany kurs oraz trend |
| `change` (Zmiana kursu) | Zmiana w ciągu ostatnich `days` dni roboczych EBC (1-20, domyślnie 1) |
| `check-alert` (Sprawdź alert) | Porównuje kurs z progiem `threshold` (`condition`: `above` albo `below`) i powiadamia raz przy każdym przekroczeniu |
| `forget` (Zapomnij zapisane dane) | Usuwa zapisane kursy i stan wszystkich alertów oraz czyści zmienne |

`base` to kod ISO 4217 (domyślnie `EUR`), `quote` inny (`USD`, `PLN`, `GBP`,
`CHF`, `JPY` …). Para bez euro (`USD`/`PLN`) to **kurs krzyżowy**, który
Frankfurter wylicza z dwóch kursów euro EBC; wtedy `cross` ma wartość `true`
i komunikat to mówi. Kwoty i progi wpisuje się jako tekst, z kropką albo
przecinkiem (`99.50`, `4,35`), bo parametr wtyczki nie ma rodzaju
dziesiętnego. Każda akcja kursowa przyjmuje opcjonalny `at`, czas
uruchomienia ([niżej](#odświeżanie-według-harmonogramu)).

### Wyjścia każdej akcji kursowej

| Wyjście | Rodzaj | Przykład |
| --- | --- | --- |
| `pair` | tekst | `EUR/USD` |
| `rate` | tekst | `1.1014` (tekst dziesiętny: wyjścia nie mają rodzaju dziesiętnego) |
| `rateDate` | tekst | `2026-11-16`, dzień, którego dotyczy kurs referencyjny |
| `cross` | logiczne | `false` |
| `delayed` | logiczne | zawsze `true`: dzienny kurs referencyjny, nigdy bieżący |
| `stale` | logiczne | `true`, gdy kurs nie jest na pewno aktualny |
| `source` | tekst | `ecb-frankfurter` |
| `attribution` | tekst | źródło danych (EBC przez Frankfurter) |
| `fetchedAt` | tekst | `2026-11-16T19:03:29Z` (nagłówek `Date` odpowiedzi), gdy znany |
| `ageMinutes` | liczba całkowita | gdy znany |
| `problem` | tekst | dlaczego kurs jest nieaktualny albo nie nowy: kod błędu lub `old-rate` |

### Wyjścia `rate`

`summary` (jedno zdanie, zaczyna się od `[nieaktualne]`, gdy kurs jest
nieaktualny), `base`, `quote`, `asOf`, `amount` i `converted` (z `amount`),
`previousRate`, `previousDate`, `change` (`-0.0038`), `changePercent`
(`-0.34`) i `direction` (`up`, `down` albo `flat` w granicach ±0,1%), gdy
zapisany jest poprzedni kurs, `min`, `max`, `trend` (od najstarszego
zapisanego kursu do najnowszego), `historyCount`, `historyFrom`.

### Wyjścia `change`

`summary`, `fromRate`, `fromDate`, `days`, `change`, `changePercent`,
`direction`. Gdy zapisanych kursów jest mniej niż `days` + 1, akcja kończy
się błędem `not-enough-history`.

### Wyjścia `check-alert`

`summary`, `condition`, `threshold`, `triggered` (warunek jest spełniony dla
pokazanego kursu), `crossed` (spełniony teraz, a przy poprzednim sprawdzeniu
nie), `notified`, `evaluated` (`false` dla nieaktualnego kursu) oraz
`notification`, gdy powiadomienie odrzucono albo ustawienia użytkownika
nigdzie go nie pokazały.

## Zmienne

Publikowane przez `rate`: `pair`, `rate`, `rateDate`, `changePercent`,
`summary` (teksty), `stale` (logiczna), `history` (JSON: do 32
`{"date", "rate"}`, od najstarszego). Publikowana przez `check-alert`, gdy
sprawdził warunek: `alertTriggered` (logiczna). Odczyt:
`{{ plugins.deckpilot.markets.rate }}`. Zawierają ostatnie uruchomienie,
niezależnie od pary. Gdy akcja kursowa się nie powiedzie, `stale` dostaje
wartość `true`.

## Alerty

*Sprawdź alert* porównuje najnowszy kurs referencyjny z Twoim progiem
(`above`: równy lub wyższy; `below`: równy lub niższy) i dla każdej pary,
warunku i progu pamięta, czy warunek był spełniony przy ostatnim
sprawdzeniu. Powiadamia **raz, gdy warunek zaczyna być spełniony**, a nie
przy każdym sprawdzeniu; gdy kurs wraca, alert jest ponownie uzbrajany i
następne przekroczenie znowu powiadamia. Pierwsze sprawdzenie nowego
alertu, którego warunek już jest spełniony, liczy się jako przekroczenie.
Nieaktualny kurs nie jest porównywany i niczego nie zmienia. Powiadomienie
odrzucone przez DeckPilota (limit: 5 na minutę) jest ponawiane przy
następnym sprawdzeniu. Gdzie pojawia się powiadomienie, ustala się w
ustawieniach wtyczki (*Powiadomienia na pulpicie*, *Pasek stanu*) i
DeckPilota.

Uruchamiaj go z przepływu według harmonogramu
([niżej](#odświeżanie-według-harmonogramu)). Przechowywanych jest najwyżej
64 alertów; najdawniej sprawdzane są usuwane.

## Odświeżanie według harmonogramu

Aby zmienne były aktualne albo aby sprawdzać alerty, dodaj przepływ z
**wyzwalaczem harmonogramu**, który uruchamia *Kurs wymiany* albo *Sprawdź
alert*. Kursy zmieniają się raz na dzień roboczy, więc co godzinę czy dwie w
zupełności wystarczy. Przekaż czas wyzwolenia (pole zdarzenia `firedAt`,
`RRRR-MM-DD GG:MM`) do parametru *Czas uruchomienia* (`at`) przez wejście
przepływu. Wtedy:

- zapisane kursy młodsze niż ich czas życia są używane ponownie bez
  zapytania. Czas życia to `Cache-Control: max-age` samej odpowiedzi
  (Frankfurter ustawia go na czas do spodziewanych nowych kursów, wieczorem
  dnia roboczego ok. pięć godzin), w granicach od 30 minut do 12 godzin, a
  bez niego 4 godziny;
- po odpowiedzi „za dużo zapytań” (HTTP 429) nic nie jest wysyłane, dopóki
  nie minie podany `Retry-After` (15 minut, jeśli go brak).

## Pamięć podręczna i czas

Każda para to jedno zapytanie
`https://api.frankfurter.dev/v2/providers/ecb/rates?base=EUR&quotes=USD&from=<31 dni przed najnowszym znanym dniem>`,
które przynosi kursy z ok. 22 dni roboczych; są one łączone z zapisaną
historią pary (najwyżej 32 kursy, żaden starszy niż 92 dni przed
najnowszym). Pierwsze zapytanie o parę, gdy żadna data nie jest jeszcze
znana, najpierw pyta tylko o najnowszy kurs. Przechowywanych jest najwyżej
32 par.

W API wtyczek 1.0 wtyczka **nie ma zegara**. Skutki, wprost:

- **Naciśnięcie klawisza bez `at` zawsze wysyła jedno zapytanie**, nawet gdy
  zapisane kursy są świeże, i nie może uszanować `Retry-After` (nie wie, kiedy
  minie). DeckPilot ogranicza wtyczkę do 60 zapytań na minutę; Frankfurter
  nie podaje limitu.
- **Bez sieci** albo po błędzie zapisane kursy są pokazywane z `stale: true`
  i czasem pobrania, chyba że ich wiek jest znany (z nagłówka `Date`
  odpowiedzi z błędem albo z `at`) i mieści się w czasie życia. Bez
  zapisanych kursów akcja kończy się kodem błędu. Zapisane kursy nigdy nie
  są pokazywane dla nieznanej waluty ani gdy uprawnienie do sieci jest
  odebrane.
- Najnowszy kurs starszy o ponad 5 dni od dnia odpowiedzi jest `stale` z
  `problem: old-rate`, nawet gdy zapytanie się powiodło: EBC przestał
  publikować tę walutę (tak było z rublem rosyjskim w 2022 r.) albo źródło
  jest opóźnione. Weekendy i święta (Wielkanoc: od czwartku do wtorku)
  mieszczą się w tych 5 dniach.

`time.now` i timery (docs/plugin-factory.md §6.2, §6.3) usunęłyby te
ograniczenia.

## Uprawnienia

| Uprawnienie | Po co |
| --- | --- |
| `network`: `api.frankfurter.dev`, `GET` | Kursy EBC par, o które pytasz |
| `notifications` (kierowane ustawieniami *Powiadomienia na pulpicie* i *Pasek stanu*) | *Sprawdź alert* |

Nic więcej. Bez danych uwierzytelniających: Frankfurter nie ma klucza, a
wtyczka żadnego nie zawiera.

## Prywatność

- **Frankfurter otrzymuje** dwa kody walut i datę początkową, wraz z Twoim
  adresem IP jak przy każdym zapytaniu, oraz `User-Agent:
  DeckPilot-Markets/0.1.0`. Nigdy kwoty ani progu.
- **Na tym komputerze** wtyczka przechowuje ostatnie kursy każdej pary oraz
  dla każdego alertu jego parę, warunek, próg i ostatni stan (pamięć wtyczki
  DeckPilota). *Zapomnij zapisane dane* usuwa to wszystko; odinstalowanie
  wtyczki również.
- Kwoty i progi są Twoje: pojawiają się w komunikacie akcji, wyjściach i
  powiadomieniu alertu, **nigdy w dzienniku**.

## Warunki Frankfurtera i EBC

Przeczytane 16.11.2026; przeczytaj je ponownie, zanim na tym polegasz.
Cytaty i szczegóły: [README.md](README.md#frankfurters-and-the-ecbs-terms).

- **Frankfurter**: open source (MIT); bez limitów miesięcznych i dziennych,
  zapytania ograniczane przeciw nadużyciom; lokalne przechowywanie dziennych
  obserwacji jest zalecane, by ograniczyć ruch; bez gwarancji, kursy mogą
  być spóźnione, brakujące lub poprawione. Kursy nie należą do Frankfurtera:
  obowiązują warunki dostawcy. Wtyczka używa tylko trasy dostawcy EBC
  (`/v2/providers/ecb/rates`), a nie domyślnej `/v2/rates`, która łączy kilku
  dostawców.
- **EBC**: informacje można swobodnie wykorzystywać, jeśli są przedstawione
  dokładnie, a EBC jest podany jako źródło; zmiany trzeba wyraźnie zaznaczyć.
  Kursy referencyjne są aktualizowane zwykle ok. 16:00 CET w każdy dzień
  roboczy poza dniami zamknięcia TARGET, publikowane wyłącznie w celach
  informacyjnych, a ich użycie do transakcji jest zdecydowanie odradzane.
  Wtyczka podaje EBC w każdym komunikacie i w `attribution` oraz mówi, gdy
  wartość jest wyliczona: kurs krzyżowy (przez Frankfurter), przeliczenie,
  zmiana (przez wtyczkę). **Jeśli pokazujesz wyjścia gdzie indziej, zachowaj
  przy nich źródło.**

## Kryptowaluty (odłożone)

Ceny kryptowalut z CoinGecko, z własnym darmowym kluczem *Demo* użytkownika
w miejscu na dane uwierzytelniające, były planowane i **nie ma ich w tej
wersji**. Warunki API CoinGecko (wersja z 5 września 2025 r.) przeczytane
16.11.2026:

- §7.1: produkt korzystający z API, który jest udostępniany innym spoza
  Twojej organizacji, wymaga wiążącej umowy z użytkownikiem i polityki
  prywatności, które przyznają prawa CoinGecko, wyłączają jego
  odpowiedzialność i czynią twórcę produktu wyłącznie odpowiedzialnym za
  ostrzeżenia, w tym o zmienności rynków kryptowalut. Wtyczka
  rozpowszechniana przez DeckPilota jest takim produktem, a DeckPilot nie ma
  takiej umowy. To decyzja właściciela, nie wtyczki.
- §6: przechowywanie danych „nie jest zalecane”, zapisane dane powinny być
  silnie szyfrowane i odświeżane co najmniej co 24 godziny, a §6.2 zabrania
  ich przechowywania i wyprowadzania z nich czegokolwiek poza dozwolonym
  zakresem, co koliduje z historią i min/max/trendem tej wtyczki.
- Warunki nie mówią nic o własnym kluczu użytkownika w cudzej aplikacji;
  strona cennika nazywa Demo darmowym planem „do wypróbowania API” (100
  zapytań na minutę, 10 000 miesięcznie) z wymaganym oznaczeniem „Powered by
  CoinGecko”.

Co trzeba by dodać po decyzji właściciela: opisuje to
[README.md](README.md#cryptocurrencies-deferred).

## Notowania akcji (niedostępne)

Przegląd dostawców ([docs/plugin-factory.md](../../docs/plugin-factory.md)
§5) nie znalazł dostawcy, którego warunki pozwalają pokazywać notowania w
rozpowszechnianej aplikacji: darmowy plan Twelve Data służy do „internal
non-display usage”; Stooq nie ma API ani warunków, a jego `robots.txt`
zabrania wszystkiego, więc byłby to scraping; Alpha Vantage i Finnhub
wymagają klucza użytkownika i służą do użytku osobistego, niekomercyjnego.
Dostawca akcji musiałby mieć: klucz użytkownika w miejscu na dane
uwierzytelniające (wtyczka nigdy nie zawiera klucza), warunki pozwalające na
wyświetlanie w aplikacji, którą użytkownik uruchamia dla siebie, notowania z
czasem giełdowym i opóźnieniem podanymi przez dostawcę, limity, które da się
uszanować pamięcią podręczną, i dokładne hosty https.

## Błędy

| Kod | Znaczenie |
| --- | --- |
| `unknown-currency` | EBC nie publikuje kursu referencyjnego tej waluty albo Frankfurter jej nie zna |
| `invalid-amount` | Kwota nie jest liczbą dodatnią |
| `invalid-threshold` | Próg nie jest liczbą dodatnią |
| `not-enough-history` | Zapisanych kursów jest mniej niż żądanych dni roboczych |
| `provider-error` | Frankfurter odpowiedział błędem i nie można było pokazać zapisanych kursów |
| `invalid-response` | Nie można odczytać odpowiedzi Frankfurtera i nie można było pokazać zapisanych kursów |
| `response-too-large` | Odpowiedź Frankfurtera była większa, niż wtyczka przyjmuje (128 KiB) |
| `timed-out` | Frankfurter nie odpowiedział na czas i nie można było pokazać zapisanych kursów |

Wspólne kody, które też zgłasza: `throttled`, `not-permitted`,
`invalid-parameters`, `dns-failed`, `connection-failed`, `tls-failed`,
`request-failed`.

## Ustawienia

*Język tekstów* (`en` albo `pl`) komunikatów i powiadomień; komunikaty
błędów są po angielsku. *Powiadomienia na pulpicie* i *Pasek stanu* kierują
alerty. Nazwy i opisy w aplikacji są po angielsku, bo manifest wtyczki nie
ma jeszcze tłumaczeń; polskie są w [locales/pl.json](locales/pl.json).

## Rozwój

```sh
cd first-party
cargo test --locked -p deckpilot-markets
cargo build --locked -p deckpilot-markets --target wasm32-unknown-unknown --profile guest
```

Testy jednostkowe używają przygotowanych odpowiedzi Frankfurtera i nigdy nie
łączą się z siecią. Ikona (`assets/icon-128.png`) to oryginalna grafika
wykonana dla tej wtyczki.
