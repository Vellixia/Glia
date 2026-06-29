import { auth } from "@/auth";

export default auth((req) => {
  const { nextUrl, auth: session } = req;
  const isLoggedIn = !!session?.user;
  const isLoginPage = nextUrl.pathname.startsWith("/login");
  const isDashboard =
    nextUrl.pathname === "/" ||
    nextUrl.pathname.startsWith("/overview") ||
    nextUrl.pathname.startsWith("/agents") ||
    nextUrl.pathname.startsWith("/skills") ||
    nextUrl.pathname.startsWith("/catalog") ||
    nextUrl.pathname.startsWith("/secrets") ||
    nextUrl.pathname.startsWith("/logs") ||
    nextUrl.pathname.startsWith("/settings") ||
    nextUrl.pathname.startsWith("/sync");

  if (isDashboard && !isLoggedIn) {
    return Response.redirect(new URL("/login", nextUrl));
  }

  if (isLoginPage && isLoggedIn) {
    return Response.redirect(new URL("/overview", nextUrl));
  }

});

export const config = {
  matcher: [
    "/((?!api|_next/static|_next/image|favicon.ico|.*\\.(?:svg|png|jpg|ico)$).*)",
  ],
};
