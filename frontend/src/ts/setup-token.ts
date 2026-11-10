/**
 * The address without the setup token, for `history.replaceState`: the same
 * path, the query without `token`, and the fragment.
 */
export function withoutToken(href: string): string {
  const url = new URL(href, "http://setup.invalid");
  url.searchParams.delete("token");
  return url.pathname + url.search + url.hash;
}

/** The setup token in the address's query, if any. */
export function tokenFromHref(href: string): string | null {
  const token = new URL(href, "http://setup.invalid").searchParams.get("token");
  return token?.trim() ? token.trim() : null;
}
