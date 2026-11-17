# Pogoda (`deckpilot.weather`)

Aktualna pogoda i prognoza do siedmiu dni dla jednego miejsca: jako
komunikat, jako wyjścia dla przepływów pracy i jako zmienne. Dane pochodzą
z [Open-Meteo](https://open-meteo.com/), bez konta i bez klucza.

[Dane pogodowe: Open-Meteo.com](https://open-meteo.com/) (CC BY 4.0).
Dane o miejscach na podstawie [GeoNames](https://www.geonames.org/).

English: [README.md](README.md).

> **Stan.** Wersja 0.1.0 jest budowana, testowana i pakowana przez CI i
> **nie jest opublikowana**. Nie da się jej jeszcze zainstalować z rejestru
> ([first-party/README.md](../README.md)). Przed publikacją właściciel musi
> potwierdzić w Open-Meteo, że z darmowego API może korzystać aplikacja
> rozpowszechniana wśród wielu użytkowników
> ([niżej](#warunki-open-meteo)).

## Na klawiszu

*Od wersji 0.2.0; wymaga DeckPilota z API wtyczek 1.2.* Klawisz przypisany
do akcji **Aktualna pogoda** pokazuje temperaturę i warunki, na przykład
`12 C` nad `Czesciowe zachmurzenie`, na kolorze dla rodzaju pogody:
bursztynowym dla słońca, fioletowym dla bezchmurnej nocy, szarym dla
chmur i mgły, niebieskim dla deszczu, morskim dla śniegu, czerwonym dla
burzy.

Klawisz pokazuje **ostatni raport pobrany przez uruchomienie akcji**
(naciśnięcie klawisza albo przepływ z harmonogramem). Samo pokazywanie
nigdy nie pyta Open-Meteo. Gdy raport jest starszy niż jego czas ważności
(15 minut), wartości zostają, ale klawisz to mówi: znacznik ostrzeżenia i
wiek jako plakietka (`40m`, `3h`, `2d`), albo `?`, gdy wiek nie jest
znany. Nigdy nie jest pokazywany jako aktualny. Naciśnij klawisz albo
odświeżaj według harmonogramu ([niżej](#odświeżanie-według-harmonogramu)),
aby pobrać nowy.

Przed pierwszym raportem albo gdy miejsce nie jest ustawione, klawisz
pokazuje nadany przez Ciebie tytuł. Teksty na klawiszach są w czcionce
DeckPilota, która nie ma `°` ani polskich liter: `12 C` i `Mgla` zamiast
`Mgła`. Komunikat i wyjścia akcji zachowują pełny tekst. Urządzenie bez
wyświetlaczy nic nie pokazuje.

## Czego wtyczka nie robi

API wtyczek 1.2 pozwala wtyczce opisać, co pokazuje klawisz (wyżej), ale
nie rysować na nim, uruchamiać się samodzielnie ani mieć timera. Dlatego
ta wersja **nie**:

- **rysuje ikony pogody.** Wbudowane symbole klawiszy DeckPilota nie mają
  słońca ani chmury; kolor oznacza rodzaj pogody. Wyjścia `conditionName`
  i `isDay` posłużą do wyboru ikony.
- **odświeża się sama.** Działa, gdy naciśniesz klawisz albo gdy uruchomi
  ją przepływ pracy. Zmienne odświeża przepływ z harmonogramem
  ([niżej](#odświeżanie-według-harmonogramu)).
- **wie, jak stare są zapisane dane, gdy naciśniesz klawisz**
  ([niżej](#pamięć-podręczna-i-czas)).
- **zapisuje własnych ustawień.** Miejsce znalezione po nazwie jest
  przechowywane w pamięci wtyczki.
- wysyła powiadomień, podaje wartości godzinowych ani ostrzeżeń
  pogodowych, ani nie używa innego dostawcy niż Open-Meteo.

## Akcje

| Akcja | Co |
| --- | --- |
| `current` (Aktualna pogoda) | Warunki, temperatura, temperatura odczuwalna, wiatr i wilgotność w tej chwili |
| `forecast` (Prognoza) | Najbliższe dni (`days`: 1-7, domyślnie 3): najniższa i najwyższa temperatura, warunki, szansa opadów |
| `find-location` (Znajdź miejsce) | Wyszukuje miejsca według `query`. `select` 0 wypisuje do pięciu wyników; `select` 1-5 zapisuje wybrany wynik jako miejsce |
| `forget-location` (Zapomnij miejsce) | Usuwa zapisane miejsce, zapisaną pogodę i zapisane wyszukiwania oraz czyści zmienne |

`current` i `forecast` przyjmują opcjonalny parametr `at`, czas
uruchomienia ([niżej](#odświeżanie-według-harmonogramu)).

### Wyjścia akcji `current`

| Wyjście | Rodzaj | Przykład |
| --- | --- | --- |
| `summary` | tekst | `Warszawa: Słaba mżawka, 7 °C (odczuwalna 3 °C). Wiatr 15 km/h S, wilgotność 92%.` |
| `condition` | tekst | `Słaba mżawka` (w języku z ustawień) |
| `conditionName` | tekst | `drizzle` (stała nazwa, [niżej](#warunki)) |
| `weatherCode` | liczba całkowita | `51` (kod WMO) |
| `isDay` | wartość logiczna | `false` |
| `temperature`, `feelsLike` | liczba całkowita | `7`, `3` |
| `temperatureUnit` | tekst | `°C` albo `°F` |
| `windSpeed`, `windUnit` | liczba całkowita, tekst | `15`, `km/h` albo `mph` |
| `windDirection`, `windCompass` | liczba całkowita, tekst | `191`, `S` (skąd wieje wiatr) |
| `humidity` | liczba całkowita | `92` (procent) |
| `units` | tekst | `metric` albo `imperial` |
| `location` | tekst | `Warszawa` |
| `observedAt` | tekst | `2026-11-16T19:15`, czas lokalny miejsca |
| `stale` | wartość logiczna | `true`, chyba że wiadomo, że dane mają mniej niż 15 minut |
| `source` | tekst | `network` albo `cache` |
| `fetchedAt` | tekst | `2026-11-16T18:16:34Z`; brak, gdy nieznany |
| `ageMinutes` | liczba całkowita | brak, gdy wtyczka nie może znać wieku danych |
| `problem` | tekst | kod błędu nieudanego żądania, gdy zamiast nowych pokazano zapisane dane |
| `attribution` | tekst | `Weather data by Open-Meteo.com (https://open-meteo.com/), CC BY 4.0` |

### Wyjścia akcji `forecast`

`summary` (wiersz na dzień), `json` (dni jako tablica JSON: `date`, `min`,
`max`, `weatherCode`, `condition`, `conditionName`,
`precipitationProbability`), `days` (liczba dni), `temperatureUnit`,
`units`, `location` oraz dla pierwszych trzech dni `day1Date`, `day1Min`,
`day1Max`, `day1Condition`, `day1ConditionName`, `day1Precipitation` (brak,
gdy model nie podaje prawdopodobieństwa), `day2…`, `day3…`. Dalej `stale`
(chyba że wiadomo, że dane mają mniej niż godzinę), `source`, `fetchedAt`,
`ageMinutes`, `problem` i `attribution` jak wyżej.

Dzień 1 to dzień pobrania danych w danym miejscu. Wtyczka nie ma zegara,
więc nie nazywa żadnego dnia „dzisiaj”: sprawdź `day1Date`.

### Wyjścia akcji `find-location`

`count`, `matches` (ponumerowany wiersz na miejsce: nazwa, region, kraj,
strefa czasowa), `source`, `attribution`, a po wyborze `selected` i
`timezone`. Współrzędne nigdy nie są wyjściem.

## Zmienne

Publikowane przez `current`: `temperature`, `feelsLike` (liczby całkowite),
`condition`, `conditionName`, `summary` (teksty), `stale` (wartość
logiczna). Publikowane przez `forecast`: `forecast` (JSON, ta sama tablica
co wyjście `json`), `forecastSummary` (tekst), `forecastStale` (wartość
logiczna).

W przepływach pracy odczytasz je jako
`{{ plugins.deckpilot.weather.temperature }}`. Gdy akcja się nie powiedzie,
jej zmienna `stale` przyjmuje wartość prawda, a `summary` i
`forecastSummary` zaczynają się od `[nieaktualne]` (albo `[stale]` przy
języku angielskim), gdy nie wiadomo, czy dane są aktualne. Zmienne znikają
po ponownym uruchomieniu DeckPilot, do następnego uruchomienia akcji.

## Ustawianie miejsca

Jeden z dwóch sposobów:

1. **Po nazwie.** Uruchom *Znajdź miejsce* z nazwą miejscowości (na stronie
   wtyczki w DeckPilot jest przycisk uruchamiania). Akcja wypisze wyniki.
   Uruchom ją ponownie, wpisując w polu *Wybór* numer właściwego wyniku.
   Miejsce zostanie zapisane na tym komputerze.
2. **Po współrzędnych.** Wpisz *Szerokość geograficzną* i *Długość
   geograficzną* w ustawieniach wtyczki (stopnie dziesiętne; przecinek
   dziesiętny też działa), opcjonalnie *Nazwę miejsca* i *Strefę czasową*.
   Gdy oba pola są ustawione, mają pierwszeństwo przed zapisanym miejscem.

Pozostałe ustawienia: *Jednostki* (`metric`: °C i km/h; `imperial`: °F i
mph) oraz *Język tekstów* (`en` albo `pl`) dla warunków i komunikatów.
Komunikaty błędów są po angielsku. Nazwy i opisy w aplikacji są po
angielsku, bo manifest wtyczki nie ma jeszcze tłumaczeń; polskie teksty są
w [locales/pl.json](locales/pl.json).

## Odświeżanie według harmonogramu

Aby zmienne były aktualne, dodaj przepływ pracy z **wyzwalaczem
harmonogramu** (co 15 minut; częściej nie ma sensu), który uruchamia
*Aktualną pogodę* albo *Prognozę*. Czas wyzwolenia (pole zdarzenia
`firedAt`, `RRRR-MM-DD GG:MM`) przekaż przez wejście przepływu do parametru
*Czas uruchomienia* (`at`). To jedyny zegar, jaki wtyczka może dostać,
zanim zapyta sieć; dzięki niemu pamięć podręczna działa zgodnie z
zamierzeniem:

- odpowiedź młodsza niż 15 minut (`current`) albo godzina (`forecast`) jest
  używana ponownie bez żądania;
- gdy Open-Meteo odpowie „za dużo żądań” (HTTP 429), wtyczka nie wysyła
  żądań, dopóki nie minie czas, o który poprosił dostawca (15 minut, jeśli
  go nie podał).

## Pamięć podręczna i czas

Jedno żądanie przynosi aktualne warunki i siedem dni naraz; odpowiedź jest
zapisywana (kilka kilobajtów, w jednostkach metrycznych) w pamięci wtyczki,
z czasem z nagłówka `Date` odpowiedzi.

W API wtyczek 1.0 wtyczka **nie ma zegara**. Wiek zapisanych danych zna
więc tylko

- przed żądaniem, gdy uruchomienie ma `at` (wyżej): porównuje je z `at`
  uruchomienia, które pobrało dane. To czasy lokalne z dokładnością do
  minuty: przy zmianie czasu (letni/zimowy) wiek jest raz błędny o wielkość
  przesunięcia, a wiek, który wyszedłby ujemny, uznaje się za nieznany;
- po żądaniu, na które Open-Meteo odpowiedział błędem: z nagłówka `Date`
  tej odpowiedzi.

Konsekwencje, wprost:

- **Naciśnięcie klawisza bez `at` zawsze wysyła jedno żądanie**, niezależnie
  od tego, jak świeże są zapisane dane. Dla niego nie da się wymusić
  15 minut. DeckPilot ogranicza wtyczkę do 60 żądań na minutę.
- **Brak sieci** (brak połączenia, przekroczenie czasu, nieczytelna albo
  zbyt duża odpowiedź, status błędu): wtyczka pokazuje zapisane dane ze
  `stale: true`, czasem ich pobrania i wiekiem, jeśli da się go ustalić
  jednym z dwóch sposobów powyżej. Bez zapisanych danych dla tego miejsca
  akcja kończy się błędem z odpowiednim kodem. Zapisane dane nigdy nie są
  pokazywane dla innego miejsca ani wtedy, gdy Open-Meteo odrzuca samo
  żądanie (HTTP 400) albo gdy odmówiono uprawnienia sieciowego.
- Dane są nazywane świeżymi (`stale: false`) tylko zaraz po udanym żądaniu
  albo gdy wiadomo, że ich wiek mieści się w czasie ważności.
- Wyszukiwania miejsc są zapisywane (ostatnie 20) i używane ponownie bez
  żądania. Wtyczka pyta o nie ponownie dopiero wtedy, gdy zobaczy odpowiedź
  datowaną ponad 30 dni później.

`time.now`, możliwość proponowana dla DeckPilot
([docs/plugin-factory.md](../../docs/plugin-factory.md) §6.3), usunęłaby te
ograniczenia: każde uruchomienie najpierw znałoby wiek danych, minimalny
wiek obowiązywałby także przy naciśnięciu klawisza, przerwa po HTTP 429
obowiązywałaby każde uruchomienie, wiek danych bez sieci byłby zawsze
podawany, a parametr `at` nie byłby potrzebny.

## Warunki

Open-Meteo podaje kod pogody WMO. Wtyczka zamienia go na tekst (angielski
albo polski) i jedną z nazw: `clear`, `mostly-clear`, `partly-cloudy`,
`overcast`, `fog`, `drizzle`, `freezing-drizzle`, `rain`, `freezing-rain`,
`snow`, `rain-showers`, `snow-showers`, `thunderstorm`,
`thunderstorm-hail`, `unknown`.

| Kody | Nazwa | Teksty |
| --- | --- | --- |
| 0 | `clear` | Bezchmurnie |
| 1 | `mostly-clear` | Przeważnie bezchmurnie |
| 2 | `partly-cloudy` | Częściowe zachmurzenie |
| 3 | `overcast` | Pochmurno |
| 45, 48 | `fog` | Mgła; Mgła osadzająca szadź |
| 51, 53, 55 | `drizzle` | Słaba, Umiarkowana, Gęsta mżawka |
| 56, 57 | `freezing-drizzle` | Słaba, Gęsta marznąca mżawka |
| 61, 63, 65 | `rain` | Słaby, Umiarkowany, Silny deszcz |
| 66, 67 | `freezing-rain` | Słaby, Silny marznący deszcz |
| 71, 73, 75, 77 | `snow` | Słabe, Umiarkowane, Silne opady śniegu; Śnieg ziarnisty |
| 80, 81, 82 | `rain-showers` | Słabe, Umiarkowane, Gwałtowne przelotne opady deszczu |
| 85, 86 | `snow-showers` | Słabe, Silne przelotne opady śniegu |
| 95, 97 | `thunderstorm` | Burza; Silna burza |
| 96, 99 | `thunderstorm-hail` | Burza ze słabym, z silnym gradem |
| każdy inny | `unknown` | Nieznane warunki |

Wtyczka zmienia wartości Open-Meteo tylko w ten sposób: temperatury i
prędkość wiatru zaokrągla do liczb całkowitych, przelicza na °F i mph przy
jednostkach imperialnych, a kod pogody i kierunek wiatru podaje także
słownie.

## Uprawnienia

| Uprawnienie | Po co |
| --- | --- |
| `network`: `api.open-meteo.com`, `GET` | Pogoda dla Twojego miejsca |
| `network`: `geocoding-api.open-meteo.com`, `GET` | Wyszukiwanie miejsca po nazwie |
| `device`: `state` | Temperatura i warunki na klawiszach przypisanych do akcji Aktualna pogoda ([Na klawiszu](#na-klawiszu)) |

Nic więcej. Pamięć wtyczki, ustawienia, własne zmienne i dziennik nie
wymagają uprawnień; pamięć podręczna jest znacznie mniejsza niż domyślny
limit pamięci, więc wtyczka nie prosi o większy. Nie ma miejsca na klucz:
darmowe API Open-Meteo nie ma klucza i wtyczka żadnego nie zawiera.

## Prywatność

- **Open-Meteo otrzymuje** szerokość i długość geograficzną Twojego
  miejsca zaokrąglone do dwóch miejsc po przecinku (około kilometra), nazwę
  strefy czasowej oraz wyszukiwane nazwy miejsc, a także Twój adres IP, jak
  przy każdym żądaniu. Zobacz
  [warunki i politykę prywatności Open-Meteo](https://open-meteo.com/en/terms).
  Żądania mają nagłówek `User-Agent: DeckPilot-Weather/0.1.0` i nic, co
  identyfikuje Ciebie albo Twój DeckPilot.
- **Na tym komputerze** wtyczka przechowuje wybrane miejsce, ostatnią
  odpowiedź pogodową i ostatnie 20 wyszukiwań miejsc w swojej pamięci
  (zaszyfrowana baza wtyczek DeckPilot). *Zapomnij miejsce* usuwa to
  wszystko; odinstalowanie wtyczki również.
- Współrzędne to dane osobowe. Wtyczka nigdy nie zapisuje ich w dzienniku,
  komunikacie, wyjściu ani zmiennej. **Nazwa** miejsca jest w komunikatach
  i w wyjściu `location`.

## Warunki Open-Meteo

Przeczytane 2026-11-16 na <https://open-meteo.com/en/terms> i
<https://open-meteo.com/en/licence>; cytaty to słowa Open-Meteo z tego
dnia (w oryginale, po angielsku). Zanim na nich polegasz, przeczytaj je
ponownie.

- **Darmowe API tylko do użytku niekomercyjnego.** „You may only use the
  free API services for non-commercial purposes.” Przykłady użytku
  niekomercyjnego według Open-Meteo: „Private or non-profit websites or
  apps that do not have subscriptions or advertising”, „Personal home
  automation purposes”. Użytek komercyjny wymaga płatnego planu z kluczem
  API; ta wtyczka nie ma na niego miejsca. **Jeśli używasz DeckPilot
  komercyjnie, sprawdź, czy Twoje użycie jest dozwolone, albo nie używaj
  tej wtyczki.**
- **Limity.** „Less than 10'000 API calls per day, 5'000 per hour and 600
  per minute”; tabela planów dodaje 300 000 wywołań na miesiąc. „Rate
  limits are applied by IP addresses and application.” Żądanie o wiele
  wartości może liczyć się jako więcej niż jedno wywołanie (na żądanie
  prognozy tej wtyczki serwer odpowiedział `X-RateLimit-Cost: 1.1`).
- **Podanie źródła.** Dane są na licencji CC BY 4.0: „You must include a
  link next to any location Open-Meteo data are displayed”, na przykład
  „Weather data by Open-Meteo.com” z odnośnikiem do
  <https://open-meteo.com/>. Wtyczka umieszcza ten wiersz z adresem na
  końcu każdego komunikatu pogodowego i w wyjściu `attribution`. **Jeśli
  pokazujesz wyjścia albo zmienne wtyczki gdzie indziej, pokaż przy nich
  źródło.** Wyszukiwanie miejsc: „Location data based on GeoNames”.
- **Brak gwarancji.** „The accuracy and completeness of the data products
  and their uninterrupted provision are not guaranteed by Open-Meteo.”
  Open-Meteo może „block applications and IP addresses that misuse our
  service without prior notice”.

**Otwarte pytanie do właściciela.** Warunki mówią o użytkowniku API i
stosują limity „by IP addresses and application”. Nie mówią, czy aplikacja
instalowana przez wiele osób, z których każda wysyła żądania z własnego
komputera, liczy się do limitów jako jedna aplikacja, ani czy samo
rozpowszechnianie takiej aplikacji mieści się w „non-commercial use”, gdy
część jej użytkowników to firmy. Żądania nazywają aplikację
(`DeckPilot-Weather/<wersja>`), więc Open-Meteo może liczyć je razem.
**Przed publikacją tej wtyczki w rejestrze produkcyjnym trzeba zapytać
Open-Meteo.** Do tego czasu jest to wersja rozwojowa.

## Błędy

| Kod | Znaczenie |
| --- | --- |
| `no-location` | Nie ustawiono miejsca: uruchom *Znajdź miejsce* albo wpisz szerokość i długość w ustawieniach |
| `invalid-location` | Szerokości, długości albo strefy czasowej z ustawień nie da się użyć |
| `location-not-found` | Wyszukiwanie nie znalazło miejsca o tej nazwie |
| `invalid-selection` | *Wybór* jest większy niż liczba znalezionych miejsc |
| `provider-error` | Open-Meteo odpowiedział błędem i nie było zapisanych danych do pokazania |
| `invalid-response` | Odpowiedzi Open-Meteo nie udało się odczytać i nie było zapisanych danych do pokazania |
| `response-too-large` | Odpowiedź Open-Meteo była większa, niż wtyczka przyjmuje (64 KiB) |
| `timed-out` | Open-Meteo nie odpowiedział na czas i nie było zapisanych danych do pokazania |

Wspólne kody, które wtyczka także zgłasza: `throttled` (limit Open-Meteo
albo DeckPilot), `not-permitted` (brak uprawnienia sieciowego),
`invalid-parameters`, `dns-failed`, `connection-failed`, `tls-failed`,
`request-failed`.

## Rozwój

```sh
cd first-party
cargo test --locked -p deckpilot-weather
cargo build --locked -p deckpilot-weather --target wasm32-unknown-unknown --profile guest
```

Testy jednostkowe używają przygotowanych odpowiedzi Open-Meteo i nigdy nie
łączą się z siecią. Dostawca jest ukryty za cechą `provider::Provider`
(`src/provider.rs`); `src/open_meteo.rs` to jej jedyna implementacja. Inny
dostawca wymaga implementacji, swoich hostów w manifeście i, jeśli ma
klucz, miejsca na dane uwierzytelniające.

Ikona (`assets/icon-128.png`) to oryginalna grafika wykonana dla tej
wtyczki: słońce za chmurą, narysowane z kół i prostokątów.
