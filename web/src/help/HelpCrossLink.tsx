type Props = {
  topicId: string;
  onOpen: (topicId: string) => void;
  /** Varsayılan: kısa "SSS" */
  label?: string;
};

export function HelpCrossLink({ topicId, onOpen, label = "SSS" }: Props) {
  return (
    <button
      type="button"
      className="help-cross-link"
      aria-label={`Yardım: ${topicId}`}
      onClick={() => onOpen(topicId)}
    >
      {label}
    </button>
  );
}
