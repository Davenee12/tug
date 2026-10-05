// Windows pop-ups, rate-limited: a group chat going off shouldn't stack a dozen toasts.
// Up to `max` show in any `windowMs`; the rest are counted and summed up once.

export class ToastLimiter {
  private sent: number[] = [];
  private held = 0;

  constructor(
    private readonly max = 3,
    readonly windowMs = 10_000,
  ) {}

  /** May this one show now? If not, it's counted for the summary. */
  admit(now: number): boolean {
    this.sent = this.sent.filter((t) => now - t < this.windowMs);
    if (this.sent.length < this.max) {
      this.sent.push(now);
      return true;
    }
    this.held += 1;
    return false;
  }

  /** How many were held back since the last call (and start counting again). */
  takeHeld(): number {
    const held = this.held;
    this.held = 0;
    return held;
  }
}
