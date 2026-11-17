# Przerwy i nawodnienie (`deckpilot.break-reminder`)

Przypomina o zrobieniu przerwy i o piciu wody, każde w swoim odstępie,
z odkładaniem, pomijaniem, wstrzymywaniem i godzinami ciszy.

English: [README.md](README.md).

> **Najpierw przeczytaj.** Wtyczka DeckPilot nie ma zegara ani własnego
> odliczania i działa tylko wtedy, gdy działa któraś z jej akcji (API
> wtyczek 1.0). Sama z siebie nie robi więc nic: **dodajesz jeden przepływ
> pracy, który co minutę uruchamia jej akcję Takt** ([Konfiguracja](#konfiguracja)).
> Od wersji 0.2.0, z API wtyczek 1.2 DeckPilota, klawisz przypisany do
> akcji **Stan** pokazuje odliczania ([Na klawiszu](#na-klawiszu)).

## Konfiguracja

Raz w każdym profilu, w którym chcesz przypomnień. Wyzwalacze przepływów
pracy działają tylko dla aktywnego profilu i gdy konfiguracja DeckPilot
jest odblokowana.

1. Zainstaluj i włącz wtyczkę; zezwól na jej jedyne uprawnienie,
   powiadomienia.
2. Utwórz przepływ pracy, na przykład „Takt przypomnień”.
3. W panelu przepływu pracy (nic nie jest zaznaczone) dodaj **wejście**:
   klucz `firedAt`, typ tekst, wymagane.
4. Dodaj **wyzwalacz**: harmonogram, co **1** minutę. Dla wejścia
   `firedAt` wybierz pole zdarzenia **Date and time** (data i godzina).
5. Za węzłem startowym dodaj jeden węzeł akcji: **Break & Hydration ›
   Tick**. Jego parametr **Date and time** zmapuj na wyjście `firedAt`
   węzła startowego (czyli na wejście przepływu pracy).
6. Zapisz. Pierwszy takt uruchamia oba odliczania; nic się nie pokaże,
   dopóki któreś się nie skończy.

Nazwy w aplikacji są po angielsku, bo API wtyczek 1.0 nie ma tłumaczeń
metadanych; polskie odpowiedniki są w [`locales/pl.json`](locales/pl.json).

W zapisanym profilu istotne części wyglądają tak (węzeł akcji wskazuje
akcję typu `deckpilot.break-reminder.tick`):

```json
{
  "inputs": [{ "key": "firedAt", "label": "Date and time", "kind": "text", "required": true }],
  "triggers": [
    { "type": "schedule", "schedule": { "every": "interval", "minutes": 1 },
      "inputs": { "firedAt": { "type": "event", "field": "firedAt" } } }
  ],
  "graph": { "nodes": [
    { "id": "start", "type": "trigger" },
    { "id": "tick", "type": "action", "action": "<akcja Tick>",
      "mappings": { "firedAt": { "type": "template", "template": "{{ steps.start.output.firedAt }}" } } }
  ] }
}
```

Data i godzina wyzwalacza (`RRRR-MM-DD GG:MM`, czas lokalny komputera) to
jedyny zegar wtyczki. Harmonogram rzadszy niż co minutę też działa,
z przypomnieniami spóźnionymi najwyżej o tyle, pod warunkiem że jest
**krótszy niż ustawienie „Away after”** (domyślnie 5 minut): dłuższa pauza
między dwoma taktami jest uznawana za przerwę i zaczyna odliczania od nowa.

Pozostałe akcje przypisz do klawiszy według uznania: Snooze (Odłóż), Skip
next (Pomiń następne), Pause or resume (Wstrzymaj lub wznów), Restart
countdown (Zacznij odliczanie od nowa), Status (Stan).

## Akcje

| Akcja | Co robi |
| --- | --- |
| `tick` (Takt) | Odejmuje od obu odliczań minuty, które minęły od ostatniego taktu, i pokazuje przypomnienia, na które przyszła pora. Parametr `firedAt`: data i godzina wyzwalacza. Dla przepływu pracy, nie dla klawisza |
| `snooze` (Odłóż) | Pokazuje przypomnienie ponownie za `minutes` minut (1-120, domyślnie 10). `reminder`: `last` (pokazane ostatnio, domyślnie), `break` (przerwa), `hydration` (woda) albo `both` (oba) |
| `skip` (Pomiń następne) | Pomija następne przypomnienie jeden raz: gdy przyjdzie na nie pora, nic się nie pokaże, a odliczanie zacznie się od nowa. `reminder`: `break` (domyślnie), `hydration`, `both` |
| `restart` (Zacznij odliczanie od nowa) | Zaczyna odliczanie od pełnego odstępu, po przerwie zrobionej z własnej inicjatywy. `reminder`: `both` (domyślnie), `break`, `hydration` |
| `pause` (Wstrzymaj) | Zatrzymuje odliczania i przypomnienia do wznowienia albo na `minutes` minut (0-1440; 0, domyślnie: do wznowienia) |
| `resume` (Wznów) | Kontynuuje odliczania od miejsca, w którym się zatrzymały |
| `toggle-pause` (Wstrzymaj lub wznów) | Jeden klawisz do obu |
| `status` (Stan) | Podaje stan według ostatniego taktu; niczego nie zmienia |

Każda akcja odpowiada komunikatem (po angielsku), na przykład `Break in
12 min, water in 22 min`, i wyjściami dla kolejnych węzłów przepływu
pracy: `state` (`waiting`: jeszcze bez taktu, `running`: działa, `paused`:
wstrzymane, `quiet`: godziny ciszy), `breakRemaining` i
`hydrationRemaining` (minuty; brak, gdy dane przypomnienie jest
wyłączone). `tick` dodaje `reminder` (`none`, `break`, `hydration`, `both`:
na co przyszła pora), `notified` oraz `reason`, gdy przypomnienie nie
zostało pokazane. Akcje wstrzymywania dodają `paused`. `status` dodaje
`paused`, `quietHours`, `pauseRemaining`, `breaksToday`, `hydrationToday`
i `lastTick`.

Te same trzy wartości są po każdej akcji publikowane jako zmienne dla
przepływów pracy: `{{ plugins.deckpilot.breakReminder.state }}`,
`{{ plugins.deckpilot.breakReminder.breakRemaining }}` i
`{{ plugins.deckpilot.breakReminder.hydrationRemaining }}`. DeckPilot
trzyma zmienne wtyczek tylko w pamięci; wracają po pierwszym takcie.

## Ustawienia

| Ustawienie | Domyślnie | |
| --- | --- | --- |
| Break reminders (Przypomnienia o przerwie) | włączone | |
| Minutes between breaks (Minuty między przerwami) | 50 | 5-240 |
| Hydration reminders (Przypomnienia o wodzie) | włączone | |
| Minutes between water (Minuty między przypomnieniami o wodzie) | 60 | 5-480 |
| Quiet hours from / until (Godziny ciszy od / do) | puste | Godziny takie jak `22:00` i `07:00`; mogą przechodzić przez północ. W tym czasie nic się nie pokazuje, a odliczania czekają |
| Away after (Nieobecność po), minuty | 5 | 3-120. Tak długa pauza między dwoma taktami liczy się jako przerwa |
| Language of reminders (Język przypomnień) | `en` | `en` albo `pl`: wtyczka nie może poznać języka aplikacji. **Wybierz `pl`, aby przypomnienia były po polsku** |
| Desktop notifications (Powiadomienia na pulpicie) | włączone | DeckPilot pokazuje przypomnienia jako powiadomienia systemu operacyjnego |
| Status bar (Pasek stanu) | włączone | DeckPilot wymienia je także na pasku stanu, gdy jest otwarty |

Zmiana odstępu albo włączenie przypomnienia zaczyna dane odliczanie od nowa.

## Jak się zachowuje

- **Dokładność to jeden takt.** Przypomnienie przychodzi z pierwszym
  taktem o jego porze lub po niej. Odkładanie, pomijanie, wstrzymywanie
  i ponowne uruchamianie działają od razu z klawisza, bez znajomości
  godziny; ich minuty liczą kolejne takty, więc „10 minut” to od 9 do 11.
- **Najwyżej jedno powiadomienie na takt.** Gdy pora na oba przypomnienia
  wypada w tej samej minucie, jedno powiadomienie wymienia oba.
- **Po uśpieniu bez serii.** DeckPilot nie nadrabia uruchomień
  harmonogramu pominiętych, gdy komputer spał, a DeckPilot był zablokowany
  lub nie działał. Pierwszy takt po pauzie trwającej co najmniej tyle, ile
  wynosi „Away after”, nic nie pokazuje i zaczyna oba odliczania od nowa:
  nieobecność była przerwą. Krótsza pauza jest wliczana, a przypomnienie,
  na które w jej trakcie przyszła pora, pokazuje się raz.
- **Godziny ciszy** zatrzymują odliczania. Przypomnienie, na które
  przyszła pora, a które nie zostało jeszcze pokazane, pokazuje się po
  ich zakończeniu.
- **Cofnięcie zegara** (ręczne albo koniec czasu letniego) niczego nie
  odejmuje od odliczań i niczego nie pokazuje; odliczania idą dalej od
  nowej godziny. Przesunięcie zegara o godzinę do przodu jest traktowane
  jak każda pauza tej długości.
- **Wstrzymanie** zatrzymuje wszystko. Wstrzymanie na czas kończy się
  z taktem, przy którym miną jego minuty.
- **Powiadomienie odrzucone przez DeckPilot** (więcej niż 5 na minutę od
  tej wtyczki albo błąd) pozostaje do pokazania i jest ponawiane przy
  następnym takcie; `tick` podaje to w `reason`. Powiadomienie, którego
  Twoje ustawienia powiadomień nigdzie nie pokazują, liczy się jako
  pokazane.

## Na klawiszu

*Od wersji 0.2.0; wymaga DeckPilota z API wtyczek 1.2.* Klawisz przypisany
do akcji **Stan** pokazuje:

| Kiedy | Tytuł | Drugi wiersz | Kolor |
| --- | --- | --- | --- |
| Odliczanie | `Przerwa 12 min` | `Woda 4 min` | morski; bursztynowy, gdy któreś przypomnienie przypada teraz (`Przerwa teraz`) |
| Wstrzymane | `Pauza` | `jeszcze 40 min` albo `do wznowienia` | szary |
| Godziny ciszy | `Godziny ciszy` | odliczania | fioletowy |
| Przed pierwszym taktem | `Czeka na takt` | odliczania | brak, z `?` |

Wyłączone przypomnienie jest pomijane; oba wyłączone: `Wylaczone`. W
trakcie odliczania pasek pokazuje, ile minęło z odliczania do bliższego
przypomnienia, a u góry biegnie znacznik. Teksty są w języku z
ustawień, bez polskich liter (czcionka klawiszy DeckPilota ich nie ma);
po angielsku: `Break 12 min`, `Water 4 min`, `Paused`, `Quiet hours`.

Klawisz pokazuje minuty **z ostatniego taktu**: odliczania przesuwają się
tylko wtedy, gdy przepływ co minutę uruchamia Takt. Gdy powinny biec, ale
od 3 minut nie było taktu (przepływ wyłączony, komputer uśpiony, DeckPilot
zablokowany), klawisz nie udaje, że liczy: dodaje znacznik ostrzeżenia i
czas od ostatniego taktu jako plakietkę (`12m`, `2h`). Nic zapisanego się
nie zmienia; następny takt doprowadza odliczania do porządku jak zawsze.

Naciśnięcie klawisza uruchamia Stan, który niczego nie zmienia; **Wstrzymaj
lub wznów** albo **Odłóż** umieść na innym klawiszu albo na długim
naciśnięciu tego. Urządzenie bez wyświetlaczy nic nie pokazuje.

## Uprawnienia i dane

- **Powiadomienia**: do pokazywania przypomnień tam, gdzie wskazują dwa
  ustawienia powyżej.
- **Stan klawisza** (`device`, `state`): do pokazywania odliczań na
  klawiszach przypisanych do akcji Stan ([Na klawiszu](#na-klawiszu)).
  Wtyczka opisuje klawisz; rysuje go DeckPilot.
- Bez dostępu do sieci, bez poświadczeń.
- Wtyczka trzyma w swojej pamięci na tym komputerze jedną małą wartość:
  oba odliczania, wstrzymanie, czas ostatniego taktu oraz liczniki
  przypomnień pokazanych, pominiętych i odłożonych. Nic nie opuszcza
  komputera, a DeckPilot usuwa te dane przy odinstalowaniu wtyczki.
- Do dziennika trafia wiersz, gdy powiadomienie zostało odrzucone, gdy
  długa pauza zaczęła odliczania od nowa, gdy zegar się cofnął i gdy
  zapisanego stanu nie dało się użyć. Żaden wiersz nie zawiera parametru
  ani ustawienia.

## Błędy

| Kod | Znaczenie |
| --- | --- |
| `invalid-fired-at` | Czas przekazany do akcji Takt nie jest datą i godziną wyzwalacza (`RRRR-MM-DD GG:MM`): sprawdź kroki 4 i 5 konfiguracji |
| `invalid-settings` | Godziny ciszy nie są godzinami takimi jak `22:00` albo ustawiono tylko jedną z dwóch |
| `storage-unavailable` | Nie udało się odczytać lub zapisać stanu przypomnień; nic nie zostało pokazane, a następny takt spróbuje ponownie |
| `invalid-parameters`, `not-permitted`, `storage-quota` | Wspólne kody DeckPilot: brak parametru lub wartość spoza zakresu; wtyczka jest wyłączona albo cofnięto jej uprawnienie do powiadomień; pamięć wtyczki jest pełna |

## Czego nie robi

Nie w tej wersji, bo API wtyczek nie ma do tego wywołania:

- **Brak własnego zegara.** Bez przepływu pracy nic się nie dzieje,
  a wtyczka nie może utworzyć go za Ciebie.
- **Brak wykrywania bezczynności.** Wtyczka nie wie, czy jesteś przy
  klawiaturze; za nieobecność liczy się tylko pauza między taktami.
- **Brak dźwięku i przycisków** w powiadomieniu: odkładanie i pomijanie
  to klawisze albo węzły przepływu pracy.
- **Jeszcze nie do zainstalowania z Marketplace**: DeckPilot przyjmuje
  wydawcę `deckpilot` tylko we wtyczkach dołączonych do aplikacji
  ([docs/plugin-factory.md](../../docs/plugin-factory.md), B6).

## Budowanie i testy

Zobacz [`first-party/README.md`](../README.md#commands). Logika
odliczania to zestaw czystych funkcji w `src/lib.rs` (`State::advance`),
testowanych minuta po minucie w `src/tests.rs`.
