import { createBrowserRouter } from "react-router-dom";
import { RootGate } from "./RootGate";
import { LoginPage } from "../pages/LoginPage";
import { SetupPage } from "../pages/SetupPage";
import { AdminShell } from "../pages/admin/AdminShell";
import { DashboardPage } from "../pages/admin/DashboardPage";
import { UsersPage } from "../pages/admin/users/UsersPage";
import { RolesPage } from "../pages/admin/roles/RolesPage";
import { NotFoundPage } from "../pages/NotFoundPage";

export const router = createBrowserRouter([
  {
    element: <RootGate />,
    children: [
      { path: "/setup", element: <SetupPage /> },
      { path: "/login", element: <LoginPage /> },
      {
        path: "/",
        element: <AdminShell />,
        children: [
          { index: true, element: <DashboardPage /> },
          { path: "users", element: <UsersPage /> },
          { path: "roles", element: <RolesPage /> },
        ],
      },
    ],
  },
  { path: "*", element: <NotFoundPage /> },
]);
