import type { CapacitorConfig } from "@capacitor/cli";

// The native iOS and Android apps wrap the same build as the PWA (apps/client/dist).
// The app ID can't change after the first store upload.
const config: CapacitorConfig = {
  appId: "com.geauxhack.app",
  appName: "Geaux Hack",
  webDir: "dist",
  // Cream behind the web view, so there's no white flash before the first paint.
  backgroundColor: "#FDF7EC",
  ios: {
    // The page handles safe areas itself with env(safe-area-inset-*).
    contentInset: "never",
  },
  plugins: {
    CapacitorSQLite: {
      iosDatabaseLocation: "Library/CapacitorDatabase",
      // Keys live in the OS keychain/keystore; not used yet.
      iosIsEncryption: false,
      androidIsEncryption: false,
    },
  },
};

export default config;
