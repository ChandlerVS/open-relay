export interface FormProps {
  formId: string;
  apiUrl: string;
}

export function Form({ formId, apiUrl }: FormProps) {
  return (
    <div data-open-relay-form={formId}>
      <p>
        OpenRelay form <strong>{formId}</strong> &mdash; coming soon.
      </p>
      <p style={{ fontSize: "0.75rem", opacity: 0.6 }}>API: {apiUrl}</p>
    </div>
  );
}
