import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** shadcn/ui's own convention: `clsx` for conditional class composition,
 *  `tailwind-merge` so two conflicting utilities (two paddings on the same
 *  axis) resolve to the last one given rather than both landing in the
 *  class list and leaving the winner to CSS source order. */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
