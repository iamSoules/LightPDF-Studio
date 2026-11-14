import type { CapacitorConfig } from "@capacitor/cli";

const config: CapacitorConfig = {
  appId: "org.lightpdf.studio",
  appName: "LightPDF Studio",
  webDir: "dist",
  server: { androidScheme: "https" },
  android: { allowMixedContent: false, backgroundColor: "#fbf9f4" },
};

export default config;
