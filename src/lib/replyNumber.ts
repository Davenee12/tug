/**
 * Which number a text typed in a conversation goes to.
 *
 * A conversation is one name, which can hold several numbers (one person's mobile and work phone,
 * or two people saved under the same name), and its default is whoever texted last. `asked` is the
 * number chosen on purpose for it: in the To: picker, or by starting the conversation for that
 * number (New message, Text on a call, Ctrl+K). It always wins, even when the conversation hasn't
 * seen that number yet: dropping it sent the text to the default, a different number and
 * sometimes a different person, with nothing on screen to say so.
 */
export function replyNumber(
  conversation: { address: string | null; addresses: string[] },
  asked: string | undefined,
): string | null {
  return asked ?? conversation.address;
}

/** The numbers the To: picker offers: the conversation's own, with `asked` first when it's a new one. */
export function replyChoices(addresses: string[], asked: string | undefined): string[] {
  return asked && !addresses.includes(asked) ? [asked, ...addresses] : addresses;
}
