import { defaultUrlTransform } from "react-markdown";

// react-markdown's default urlTransform strips "data:" URIs (its XSS
// safelist is http/https/irc/mailto/xmpp only), which silently drops every
// inline image we embed as a base64 data URI. Allow data:image/* through
// while keeping the default sanitization for every other URL/protocol.
export function markdownUrlTransform(url: string): string {
  if (url.startsWith("data:image/")) return url;
  return defaultUrlTransform(url);
}
