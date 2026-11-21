# Szablon (`deckpilot.template`)

Punkt wyjścia dla własnej wtyczki DeckPilot. Jest budowany i testowany przy
każdym pull requeście i **nigdy nie jest publikowany**.

English: [README.md](README.md).

## Co robi

| Akcja | Sterowanie | Co |
| --- | --- | --- |
| `count` (Zlicz) | klawisz: naciśnięcie; pokrętło: obrót w prawo | Dodaje `step` (1-100, domyślnie 1) do licznika i podaje nową wartość |
| `reset` (Wyzeruj) | tylko klawisze (bez `controls`) | Ustawia licznik z powrotem na zero |

Licznik jest zapisany w pamięci wtyczki, więc przetrwa ponowne uruchomienie
DeckPilot, i jest publikowany jako zmienna
`{{ plugins.deckpilot.template.count }}` dla przepływów pracy. Ustawienie
**Etykieta** to słowo przed liczbą w komunikacie akcji.

## API wtyczek 1.3

Manifest deklaruje `"pluginApi": "^1.3"` (DeckPilot 0.8.0 lub nowszy), bo
korzysta z tego, co dodało 1.3:

- `controls` w `count`: akcję można przypisać do klawisza (naciśnięcie) i do
  pokrętła (każdy skok w prawo dodaje `step`). Nie ma obrotu w lewo, bo nic
  nie liczy w dół. Kod deklaruje to samo przez `Action::controls`, a test
  jednostkowy porównuje oba.
- Brak `controls` w `reset`: akcja bez nich działa tylko na klawiszach
  (naciśnięcie, puszczenie, kliknięcie, dwuklik, długie naciśnięcie),
  nigdy na pokrętle ani w strefie dotykowej. Tak jest dobrze dla
  większości akcji.
- `contributes.navigation` i `contributes.views`: pozycja paska narzędzi
  **Licznik** z ikoną `assets/nav-counter.svg` otwiera widok zmiennych
  wtyczki rysowany przez sam DeckPilot; wtyczka nie dostarcza kodu
  interfejsu. Ikony nawigacji to pliki PNG, WebP lub SVG w `assets/`; SVG
  ze skryptem, obsługą zdarzeń, zewnętrznym odnośnikiem itp. jest
  odrzucany przy budowaniu pakietu.

Wtyczka, która nic z tego nie potrzebuje, zostawia `"pluginApi"` na
najniższym poziomie, którego używa; `controls`, `navigation` i `views`
poniżej `^1.3` są odrzucane.

## Uprawnienia

Żadne. Pamięć wtyczki, ustawienia, własne zmienne i dziennik nie wymagają
uprawnień.

## Błędy

| Kod | Znaczenie |
| --- | --- |
| `counter-full` | Licznik osiągnął największą wartość; wyzeruj go |

## Nowa wtyczka na podstawie szablonu

Zobacz [`first-party/README.md`](../README.md#adding-a-plugin).
