"use client";

import dynamic from "next/dynamic";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";

const ChartInner = dynamic(
  () => import("recharts").then((mod) => {
    const {
      ResponsiveContainer,
      LineChart,
      BarChart,
      Line,
      Bar,
      XAxis,
      YAxis,
      CartesianGrid,
      Tooltip,
    } = mod;

    function ChartComponent({
      data,
      type,
      xKey,
      yKey,
    }: {
      data: Record<string, unknown>[];
      type: "line" | "bar";
      xKey: string;
      yKey: string;
    }) {
      if (type === "line") {
        return (
          <ResponsiveContainer width="100%" height={300}>
            <LineChart data={data}>
              <CartesianGrid strokeDasharray="3 3" />
              <XAxis dataKey={xKey} />
              <YAxis />
              <Tooltip />
              <Line
                type="monotone"
                dataKey={yKey}
                stroke="hsl(var(--chart-1))"
                strokeWidth={2}
              />
            </LineChart>
          </ResponsiveContainer>
        );
      }

      return (
        <ResponsiveContainer width="100%" height={300}>
          <BarChart data={data}>
            <CartesianGrid strokeDasharray="3 3" />
            <XAxis dataKey={xKey} />
            <YAxis />
            <Tooltip />
            <Bar dataKey={yKey} fill="hsl(var(--chart-1))" />
          </BarChart>
        </ResponsiveContainer>
      );
    }

    return { default: ChartComponent };
  }),
  {
    ssr: false,
    loading: () => <Skeleton className="h-[300px] w-full" />,
  }
);

interface ChartProps {
  title: string;
  data: Record<string, unknown>[];
  type: "line" | "bar";
  xKey: string;
  yKey: string;
}

export function DynamicChart({ title, data, type, xKey, yKey }: ChartProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
      </CardHeader>
      <CardContent>
        <ChartInner data={data} type={type} xKey={xKey} yKey={yKey} />
      </CardContent>
    </Card>
  );
}
