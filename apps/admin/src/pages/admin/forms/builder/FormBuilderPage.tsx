import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ArrowLeft, Eye, EyeOff } from "lucide-react";
import { ShadowForm, type PublicFormDto } from "@open-relay/form-renderer";
import {
  Alert,
  AlertDescription,
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  Skeleton,
} from "@open-relay/ui";
import { api } from "../../../../lib/api/client";
import { useForm } from "../../../../lib/forms/useForms";
import { useUpdateForm } from "../../../../lib/forms/useFormMutations";
import { useTheme } from "../../../../lib/theme/useTheme";
import { Canvas } from "./Canvas";
import { Inspector } from "./Inspector";
import { Palette } from "./Palette";
import { validateLayout } from "./validate";
import {
  controllerCandidates,
  elementKey,
  newCustomElement,
  newDecorationElement,
  newId,
  newStandardElement,
  renameRuleReferences,
  stripIds,
  stripRuleReferences,
  usedStandardKeys,
  withIds,
  withRule,
  type BuilderElement,
  type CustomTypeName,
  type FormElement,
  type VisibilityRule,
} from "./model";

export function FormBuilderPage() {
  const { id } = useParams<{ id: string }>();
  const formId = Number(id);
  const valid = Number.isFinite(formId);

  const { resolved: theme } = useTheme();
  const { data: form, isLoading } = useForm(valid ? formId : null);
  const update = useUpdateForm();

  const [items, setItems] = useState<BuilderElement[] | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showPreview, setShowPreview] = useState(true);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  // Seed once the form loads. `layout` is always populated by the server —
  // derived from the legacy columns for forms written before it existed.
  useEffect(() => {
    if (form && items === null) setItems(withIds(form.layout));
  }, [form, items]);

  const errors = useMemo(() => (items ? validateLayout(items) : {}), [items]);
  const errorCount = Object.keys(errors).length;
  const dirty = useMemo(() => {
    if (!form || !items) return false;
    return JSON.stringify(stripIds(items)) !== JSON.stringify(form.layout);
  }, [form, items]);

  const selectedIndex = items?.findIndex((i) => i.id === selectedId) ?? -1;
  const selected = selectedIndex >= 0 ? (items?.[selectedIndex] ?? null) : null;

  const append = (element: FormElement) => {
    const item = { id: newId(), element };
    setItems((prev) => [...(prev ?? []), item]);
    setSelectedId(item.id);
    setSavedAt(null);
  };

  const patchSelected = (element: FormElement) => {
    setItems((prev) => {
      const before = prev ?? [];
      const current = before.find((i) => i.id === selectedId);
      let next = before.map((i) => (i.id === selectedId ? { ...i, element } : i));
      // A custom field's key is editable, and rules reference keys. Repoint them
      // as it is retyped so a rename doesn't dangle every rule that depends on
      // this field and block save on an error the user can't see the cause of.
      const from = current ? elementKey(current.element) : null;
      const to = elementKey(element);
      if (from && to !== null && from !== to) {
        next = renameRuleReferences(next, from, to);
      }
      return next;
    });
    setSavedAt(null);
  };

  /** Copy one rule onto several elements at once — a branch usually hides a block. */
  const applyRuleToMany = (ids: string[], rule: VisibilityRule) => {
    const targets = new Set(ids);
    setItems((prev) =>
      (prev ?? []).map((i) =>
        targets.has(i.id) ? { ...i, element: withRule(i.element, rule) } : i,
      ),
    );
    setSavedAt(null);
  };

  const remove = (rid: string) => {
    setItems((prev) => {
      const before = prev ?? [];
      // Drop the rules that pointed at this field, so deleting a controller
      // can't strand the form in a state the server refuses to save.
      const key = before.find((i) => i.id === rid)?.element ?? null;
      const dropped = before.filter((i) => i.id !== rid);
      const referenced = key ? elementKey(key) : null;
      return referenced ? stripRuleReferences(dropped, referenced) : dropped;
    });
    if (rid === selectedId) setSelectedId(null);
    setSavedAt(null);
  };

  const save = () => {
    if (!items || errorCount > 0) return;
    setSaveError(null);
    // Only `layout` goes up — the server derives standard_fields/custom_fields
    // from it, and sending both is rejected.
    update.mutate(
      { id: formId, input: { layout: stripIds(items) } },
      {
        onSuccess: () => setSavedAt(Date.now()),
        onError: (e) => setSaveError(e.message),
      },
    );
  };

  // The schema the preview renders: the live, unsaved layout. The legacy pair
  // is carried through only to satisfy the type — the renderer prefers
  // `layout` whenever it's present.
  const previewSchema: PublicFormDto | null = useMemo(() => {
    if (!form || !items) return null;
    return {
      id: form.id,
      name: form.name,
      slug: form.slug,
      standard_fields: form.standard_fields as PublicFormDto["standard_fields"],
      custom_fields: [],
      layout: stripIds(items) as PublicFormDto["layout"],
      // Carried through so the preview shows the configured confirmation
      // rather than the built-in default. It's edited in the settings dialog,
      // not here, so it just rides along from the loaded form.
      post_submission_action: form.post_submission_action,
      // Same deal, and note the renderer's field is optional for forward
      // compatibility — omitting it here compiles fine and silently previews
      // the default bar instead of the form's real setting.
      progress_indicator: form.progress_indicator,
    };
  }, [form, items]);

  if (!valid) return <p className="text-sm text-destructive">Invalid form id.</p>;
  if (isLoading || !form || !items) {
    return (
      <div className="space-y-3">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3 flex-wrap">
        <Button variant="ghost" size="sm" asChild>
          <Link to="/forms">
            <ArrowLeft className="h-4 w-4 mr-1" />
            Forms
          </Link>
        </Button>
        <div className="flex-1 min-w-0">
          <h1 className="text-xl font-semibold truncate">{form.name}</h1>
          <p className="text-xs text-muted-foreground">
            Drag to reorder. Standard and custom fields can be freely
            interleaved.
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setShowPreview((v) => !v)}
        >
          {showPreview ? (
            <EyeOff className="h-4 w-4 mr-1" />
          ) : (
            <Eye className="h-4 w-4 mr-1" />
          )}
          {showPreview ? "Hide preview" : "Show preview"}
        </Button>
        <Button
          size="sm"
          onClick={save}
          disabled={!dirty || errorCount > 0 || update.isPending}
        >
          {update.isPending ? "Saving…" : "Save layout"}
        </Button>
      </div>

      {errorCount > 0 && (
        <Alert variant="destructive">
          <AlertDescription>
            {errorCount === 1
              ? "One element needs attention before you can save."
              : `${errorCount} elements need attention before you can save.`}
          </AlertDescription>
        </Alert>
      )}
      {saveError && (
        <Alert variant="destructive">
          <AlertDescription>{saveError}</AlertDescription>
        </Alert>
      )}
      {savedAt && !dirty && (
        <Alert>
          <AlertDescription>Layout saved.</AlertDescription>
        </Alert>
      )}

      <div
        className={
          showPreview
            ? "grid gap-4 xl:grid-cols-[minmax(0,13rem)_minmax(0,1fr)_minmax(0,17rem)_minmax(0,20rem)]"
            : "grid gap-4 lg:grid-cols-[minmax(0,13rem)_minmax(0,1fr)_minmax(0,17rem)]"
        }
      >
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Add</CardTitle>
          </CardHeader>
          <CardContent>
            <Palette
              usedStandard={usedStandardKeys(items)}
              onAddStandard={(key) => append(newStandardElement(key))}
              onAddCustom={(type: CustomTypeName) =>
                append(newCustomElement(type, items.length))
              }
              onAddDecoration={(kind) => append(newDecorationElement(kind))}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Fields</CardTitle>
          </CardHeader>
          <CardContent>
            <Canvas
              items={items}
              selectedId={selectedId}
              errors={errors}
              onSelect={setSelectedId}
              onRemove={remove}
              onReorder={(next) => {
                setItems(next);
                setSavedAt(null);
              }}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Settings</CardTitle>
          </CardHeader>
          <CardContent>
            <Inspector
                item={selected}
                onChange={patchSelected}
                candidates={controllerCandidates(items, selectedIndex)}
                ruleTargets={items.slice(selectedIndex + 1)}
                onApplyRuleToMany={applyRuleToMany}
              />
          </CardContent>
        </Card>

        {showPreview && previewSchema && (
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Preview</CardTitle>
            </CardHeader>
            <CardContent>
              {/*
                Rendered through the same ShadowForm the embed uses, fed the
                unsaved layout. previewMode disables submit so experimenting
                here can't create real submissions or fire deliveries.
              */}
              <ShadowForm
                formId={String(form.id)}
                apiUrl={api.baseUrl}
                schema={previewSchema}
                previewMode
                theme={theme}
              />
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  );
}
