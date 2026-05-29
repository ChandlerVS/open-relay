import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "react-router-dom";
import { QueryClientProvider } from "@tanstack/react-query";
import "./styles/globals.css";
import { router } from "./app/router";
import { createQueryClient } from "./lib/api/queryClient";
import { AuthProvider } from "./lib/auth/AuthProvider";
import { ThemeProvider } from "./lib/theme/ThemeProvider";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

const queryClient = createQueryClient();

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <AuthProvider>
          <RouterProvider router={router} />
        </AuthProvider>
      </ThemeProvider>
    </QueryClientProvider>
  </StrictMode>,
);
