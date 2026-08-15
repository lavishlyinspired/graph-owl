const inr = new Intl.NumberFormat("en-IN", {
  maximumFractionDigits: 0,
});

export function inrFormat(value) {
  const num = Number(value ?? 0);
  const negative = num < 0;
  const body = inr.format(Math.abs(num));
  return `${negative ? "-" : ""}₹${body}`;
}

export function amount(value) {
  const num = Number(value ?? 0);
  if (num === 0) return "—";
  return inrFormat(num);
}

export function diff(value) {
  if (value === null || value === undefined) return "—";
  return inrFormat(value);
}

export function statusLabel(status) {
  return {
    matched: "Matched",
    review: "Review",
    only_books: "Only Books",
    only_gstr2b: "Only GSTR-2B",
  }[status] ?? status;
}

export function statusColor(status) {
  return {
    matched: "text-matcha-green border-matcha-green/30 bg-matcha-green-surface",
    review: "text-matcha-amber border-matcha-amber/30 bg-matcha-amber/10",
    only_books: "text-matcha-red border-matcha-red/30 bg-matcha-red/10",
    only_gstr2b: "text-matcha-blue border-matcha-blue/30 bg-matcha-blue/10",
  }[status] ?? "text-matcha-text-secondary border-matcha-border bg-matcha-bg-secondary";
}

export function confidence(status) {
  if (status === "matched") return "100% OK";
  if (status === "review") return "Partial Match";
  return "—";
}
