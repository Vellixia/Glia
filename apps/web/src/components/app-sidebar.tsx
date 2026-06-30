"use client";

import { signOut } from "next-auth/react";
import { useRouter } from "next/navigation";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from "@/components/ui/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { ConnectionIndicator } from "@/components/connection-indicator";
import { Button } from "@/components/ui/button";
import {
  LayoutDashboard,
  Wrench,
  Store,
  Bot,
  Key,
  RefreshCw,
  ScrollText,
  Settings,
  LogOut,
} from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";

const navigation = [
  { title: "Overview", url: "/overview", testId: "nav-overview", icon: LayoutDashboard },
  { title: "Skills", url: "/skills", testId: "nav-skills", icon: Wrench },
  { title: "Catalog", url: "/catalog", testId: "nav-catalog", icon: Store },
  { title: "Agents", url: "/agents", testId: "nav-agents", icon: Bot },
  { title: "Secrets", url: "/secrets", testId: "nav-secrets", icon: Key },
  { title: "Sync", url: "/sync", testId: "nav-sync", icon: RefreshCw },
  { title: "Logs", url: "/logs", testId: "nav-logs", icon: ScrollText },
  { title: "Settings", url: "/settings", testId: "nav-settings", icon: Settings },
];

export function AppSidebar() {
  const pathname = usePathname();
  const router = useRouter();

  async function handleLogout() {
    await signOut({ redirect: false });
    router.push("/login");
  }

  return (
    <Sidebar>
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary text-primary-foreground font-bold text-sm">
            G
          </div>
          <span className="font-semibold text-lg">Glia</span>
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Navigation</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {navigation.map((item) => (
                <SidebarMenuItem key={item.url}>
                  <SidebarMenuButton
                    render={<Link href={item.url} data-testid={item.testId} />}
                    isActive={pathname === item.url}
                  >
                    <item.icon className="mr-2 h-4 w-4" />
                    <span>{item.title}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <div className="flex flex-col gap-2 px-2 py-1">
          <ConnectionIndicator />
          <div className="flex items-center justify-between">
            <ThemeToggle />
            <Button
              variant="ghost"
              size="icon"
              onClick={handleLogout}
              data-testid="logout-btn"
              aria-label="Sign out"
            >
              <LogOut className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
