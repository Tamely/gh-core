import { Link } from "../router";

/** Stand-in for screens that arrive in later PRs, so every reserved route resolves. */
export function placeholder(title: string, note: string) {
  return () => (
    <div class="flex flex-col gap-2">
      <h1 class="text-[30px] font-extrabold tracking-[-0.03em]">{title}</h1>
      <p class="text-base text-muted dark:text-muted-inverse">{note}</p>
    </div>
  );
}

export function NotFound() {
  return (
    <div class="flex flex-col gap-3">
      <h1 class="text-[30px] font-extrabold tracking-[-0.03em]">Wrong turn</h1>
      <p class="text-base text-muted dark:text-muted-inverse">
        This page fell into a pothole on Nicholson.
      </p>
      <Link href="/" class="font-bold text-beau dark:text-beau-glow">
        Back to Home
      </Link>
    </div>
  );
}
