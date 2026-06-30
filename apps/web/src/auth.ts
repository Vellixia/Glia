import NextAuth from "next-auth";
import Credentials from "next-auth/providers/credentials";
import { isProduction, APP_ENV } from "@/lib/env";

const HUB_URL = process.env.HUB_URL ?? "http://127.0.0.1:3000";

export const { handlers, signIn, signOut, auth } = NextAuth({
  // GLIA_APP_ENV=development|test|staging → trustHost=true because the
  // dashboard sits behind a reverse proxy in those environments (Host
  // header differs from canonical AUTH_URL). Production deployments must
  // explicitly set AUTH_URL or ALLOWED_HOSTS via Auth.js configuration.
  trustHost: !isProduction || APP_ENV === "test" || process.env.AUTH_URL === undefined,
  session: { strategy: "jwt" },
  pages: {
    signIn: "/login",
  },
  providers: [
    Credentials({
      credentials: {
        password: { label: "Hub Password", type: "password" },
      },
      async authorize(credentials) {
        if (!credentials?.password) return null;

        try {
          const res = await fetch(`${HUB_URL}/graphql`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              query: `mutation Login($input: LoginInput!) { login(input: $input) { token expiresAt } }`,
              variables: { input: { password: credentials.password } },
            }),
          });

          const json = await res.json();
          if (json.errors?.length) {
            console.error("[auth] hub login error:", JSON.stringify(json.errors));
            return null;
          }
          const hubToken = json?.data?.login?.token;
          if (!hubToken) return null;

          return { id: "admin", name: "Admin", hubToken };
        } catch (err) {
          console.error("[auth] hub login fetch failed:", err);
          return null;
        }
      },
    }),
  ],
  callbacks: {
    async jwt({ token, user }) {
      if (user) {
        token.hubToken = (user as any).hubToken;
      }
      return token;
    },
    async session({ session, token }) {
      session.hubToken = token.hubToken as string;
      session.user.id = token.sub ?? "admin";
      return session;
    },
  },
});
