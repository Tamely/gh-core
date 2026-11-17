import { Home } from "./pages/Home";
import { NotFound, placeholder } from "./pages/Placeholder";
import { Schedule } from "./pages/Schedule";
import { Router, type Route } from "./router";
import { TabBar } from "./ui/TabBar";

// Paths reserved in handoffs/josh.md. Placeholders are replaced in later PRs.
const routes: Route[] = [
  { path: "/", component: Home },
  { path: "/schedule", component: Schedule },
  {
    path: "/scan",
    component: placeholder("Scan", "Scanning arrives with the native app."),
  },
  {
    path: "/map",
    component: placeholder("Map", "The PFT floorplan is on its way."),
  },
  {
    path: "/battle",
    component: placeholder("Battle", "Beau vs. Parkeaux standings go here."),
  },
  {
    path: "/profile",
    component: placeholder("Profile", "Your pass and settings go here."),
  },
  {
    path: "/judge/:token",
    component: placeholder("Judging", "Judge rooms open on presentation day."),
  },
  {
    path: "/auth/callback",
    component: placeholder("Signing in", "Sign-in arrives with the auth PR."),
  },
  {
    path: "/n/:shortId",
    component: placeholder(
      "Notification",
      "Alert links arrive with notifications.",
    ),
  },
  {
    path: "/privacy",
    component: placeholder(
      "Privacy",
      "Our privacy policy will be published here.",
    ),
  },
];

export function App() {
  return (
    <>
      <main class="mx-auto max-w-md px-5 pt-[max(56px,env(safe-area-inset-top))] pb-[calc(max(16px,env(safe-area-inset-bottom))+96px)]">
        <Router routes={routes} fallback={NotFound} />
      </main>
      <TabBar />
    </>
  );
}
