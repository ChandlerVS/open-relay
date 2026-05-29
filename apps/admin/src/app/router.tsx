import { createBrowserRouter } from "react-router-dom";
import { RootGate } from "./RootGate";
import { LoginPage } from "../pages/LoginPage";
import { SetupPage } from "../pages/SetupPage";
import { OAuthCallbackPage } from "../pages/OAuthCallbackPage";
import { AdminShell } from "../pages/admin/AdminShell";
import { DashboardPage } from "../pages/admin/DashboardPage";
import { UsersPage } from "../pages/admin/users/UsersPage";
import { RolesPage } from "../pages/admin/roles/RolesPage";
import { FormsPage } from "../pages/admin/forms/FormsPage";
import { AuthSettingsPage } from "../pages/admin/settings/AuthSettingsPage";
import { ProfilePage } from "../pages/admin/profile/ProfilePage";
import { NotFoundPage } from "../pages/NotFoundPage";

export const router = createBrowserRouter([
  {
    element: <RootGate />,
    children: [
      { path: "/setup", element: <SetupPage /> },
      { path: "/login", element: <LoginPage /> },
      { path: "/oauth/callback", element: <OAuthCallbackPage /> },
      {
        path: "/",
        element: <AdminShell />,
        children: [
          { index: true, element: <DashboardPage /> },
          { path: "forms", element: <FormsPage /> },
          { path: "users", element: <UsersPage /> },
          { path: "roles", element: <RolesPage /> },
          { path: "settings/auth", element: <AuthSettingsPage /> },
          { path: "profile", element: <ProfilePage /> },
        ],
      },
    ],
  },
  { path: "*", element: <NotFoundPage /> },
]);
