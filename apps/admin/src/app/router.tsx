import { createBrowserRouter } from "react-router-dom";
import { RootGate } from "./RootGate";
import { LoginPage } from "../pages/LoginPage";
import { SetupPage } from "../pages/SetupPage";
import { OAuthCallbackPage } from "../pages/OAuthCallbackPage";
import { RequirePermissionRoute } from "../lib/auth/RequirePermissionRoute";
import { AdminShell } from "../pages/admin/AdminShell";
import { DashboardPage } from "../pages/admin/DashboardPage";
import { UsersPage } from "../pages/admin/users/UsersPage";
import { RolesPage } from "../pages/admin/roles/RolesPage";
import { FormsPage } from "../pages/admin/forms/FormsPage";
import { FormPreviewPage } from "../pages/admin/forms/FormPreviewPage";
import { FormBuilderPage } from "../pages/admin/forms/builder/FormBuilderPage";
import { BackendsPage } from "../pages/admin/backends/BackendsPage";
import { RepsPage } from "../pages/admin/reps/RepsPage";
import { SubmissionsPage } from "../pages/admin/submissions/SubmissionsPage";
import { AuthSettingsPage } from "../pages/admin/settings/AuthSettingsPage";
import { StorageSettingsPage } from "../pages/admin/settings/StorageSettingsPage";
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
        // Each resource sits behind a pathless wrapper carrying the same
        // `:read` the server's handlers require, so a typed-in URL lands on an
        // explanation instead of firing a query that can only 403. The
        // dashboard and the self-service profile are deliberately ungated —
        // `GET /dashboard` and `/auth/me` are authenticated-only server-side,
        // and a user with no permissions at all must still have somewhere to
        // land.
        children: [
          { index: true, element: <DashboardPage /> },
          { path: "profile", element: <ProfilePage /> },
          {
            element: <RequirePermissionRoute perm="forms:read" action="view forms" />,
            children: [
              { path: "forms", element: <FormsPage /> },
              { path: "forms/:id/preview", element: <FormPreviewPage /> },
              // Write is not required to *open* the builder — it renders
              // read-only without `forms:write`. See FormBuilderPage.
              { path: "forms/:id/build", element: <FormBuilderPage /> },
            ],
          },
          {
            element: (
              <RequirePermissionRoute perm="backends:read" action="view backends" />
            ),
            children: [{ path: "backends", element: <BackendsPage /> }],
          },
          {
            element: (
              <RequirePermissionRoute perm="reps:read" action="view sales reps" />
            ),
            children: [{ path: "reps", element: <RepsPage /> }],
          },
          {
            element: (
              <RequirePermissionRoute
                perm="submissions:read"
                action="view submissions"
              />
            ),
            children: [{ path: "submissions", element: <SubmissionsPage /> }],
          },
          {
            element: <RequirePermissionRoute perm="users:read" action="view users" />,
            children: [{ path: "users", element: <UsersPage /> }],
          },
          {
            element: <RequirePermissionRoute perm="roles:read" action="view roles" />,
            children: [{ path: "roles", element: <RolesPage /> }],
          },
          {
            // There is no `auth_config:read` — the server gates
            // `GET /auth/oauth/admin-config` on write too, so write is the
            // read gate for this page.
            element: (
              <RequirePermissionRoute
                perm="auth_config:write"
                action="manage authentication settings"
              />
            ),
            children: [{ path: "settings/auth", element: <AuthSettingsPage /> }],
          },
          {
            // Same shape as auth: there is no `storage_config:read`, so write
            // is the read gate for this page too.
            element: (
              <RequirePermissionRoute
                perm="storage_config:write"
                action="manage file storage"
              />
            ),
            children: [
              { path: "settings/storage", element: <StorageSettingsPage /> },
            ],
          },
        ],
      },
    ],
  },
  { path: "*", element: <NotFoundPage /> },
]);
