# Szablon (`deckpilot.template`)

Punkt wyjścia dla własnej wtyczki DeckPilot. Jest budowany i testowany przy
każdym pull requeście i **nigdy nie jest publikowany**.

English: [README.md](README.md).

## Co robi

| Akcja | Co |
| --- | --- |
| `count` (Zlicz) | Dodaje `step` (1-100, domyślnie 1) do licznika i podaje nową wartość |
| `reset` (Wyzeruj) | Ustawia licznik z powrotem na zero |

Licznik jest zapisany w pamięci wtyczki, więc przetrwa ponowne uruchomienie
DeckPilot, i jest publikowany jako zmienna
`{{ plugins.deckpilot.template.count }}` dla przepływów pracy. Ustawienie
**Etykieta** to słowo przed liczbą w komunikacie akcji.

## Uprawnienia

Żadne. Pamięć wtyczki, ustawienia, własne zmienne i dziennik nie wymagają
uprawnień.

## Błędy

| Kod | Znaczenie |
| --- | --- |
| `counter-full` | Licznik osiągnął największą wartość; wyzeruj go |

## Nowa wtyczka na podstawie szablonu

Zobacz [`first-party/README.md`](../README.md#adding-a-plugin).
