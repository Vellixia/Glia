import { DefaultSession } from "next-auth";

declare module "next-auth" {
  interface User {
    hubToken?: string;
  }
  interface Session {
    hubToken?: string;
    user: {
      id: string;
    } & DefaultSession["user"];
  }
}

declare module "next-auth/jwt" {
  interface JWT {
    hubToken?: string;
  }
}
