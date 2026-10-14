# tiny — hand-written fixture feed

An invented city ("Fixture Transit", timezone America/Toronto, dates in 2030)
so tests never depend on a real feed.

- 5 stops A–E, 3 routes, 10 trips, 2 services (WK = Mon–Fri, WE = Sat–Sun).
- Holiday 2030-01-01 (a Tuesday): WK removed, WE added.
- `R2_night` runs past midnight (arrives 24:10:00).
- `R3_express` leaves 5 min after `R3_slow` but overtakes it → 1 overtaking split.
- `R3_noon` has no time at C → interpolated to 12:15:00.
