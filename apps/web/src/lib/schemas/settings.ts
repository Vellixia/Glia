import { z } from "zod";

export const settingsSchema = z.object({
  theme: z.enum(["light", "dark", "system"]),
  logLevel: z.enum(["info", "warn", "error", "debug"]),
  hubUrl: z.string().url(),
});

export type SettingsFormValues = z.infer<typeof settingsSchema>;
