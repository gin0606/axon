//! Why an ordinary operation was refused, as a value naming the Entities involved. The moves,
//! dependency changes, conversions and lifecycle transitions carry one for each of their rules;
//! the operations that check settled current values also carry one when conflicts or a missing
//! or conflicted Entity stop them. Resolve, Notes and declarations keep plain messages. The
//! text is the core's diagnostic; callers that present the reason otherwise match on the
//! variant.
use super::record::ViolationKind;
use super::{EntityId, Kind, Lifecycle, Operation};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Conflicted Entities block every operation except resolve and note add.
    Conflicted(Vec<EntityId>),
    /// The Entity operated on, or named by the operation, is conflicted.
    EntityConflicted(EntityId),
    /// The Entity operated on, or named by the operation, is not in the store.
    Missing(EntityId),
    /// The Entity's parent is not an unfinished Group, so what is under it stays and its
    /// lifecycle does not change.
    ParentClosed(EntityId),
    /// The destination of a move or registration is not an unfinished Group.
    DestinationNotOpenGroup(EntityId),
    /// A Group was to contain itself.
    SelfContainment,
    /// The destination is inside the moved Entity.
    ContainmentCycle { destination: EntityId },
    /// Work already InProgress or Completed moves only under an adopted line; `unadopted` are
    /// the destination and its ancestors that are not adopted (NotStarted).
    StartedWorkNeedsAdoptedDestination {
        destination: EntityId,
        unadopted: Vec<EntityId>,
    },
    /// The result would add these structural violations.
    NewViolations(Vec<(EntityId, ViolationKind)>),
    /// The result would put `dependent` waiting on `predecessor` on a completion cycle.
    CompletionCycle {
        dependent: EntityId,
        predecessor: EntityId,
    },
    /// An Entity was to depend on itself.
    SelfDependency,
    /// The dependency target contains the Entity.
    DependencyOnAncestor { entity: EntityId, target: EntityId },
    /// The dependency target is inside the Entity.
    DependencyOnDescendant { entity: EntityId, target: EntityId },
    /// The dependencies of a Completed Entity are fixed.
    CompletedDependenciesFixed,
    /// An InProgress Issue is released before it is converted.
    ConvertInProgress,
    /// A Completed or Cancelled Entity is not converted.
    ConvertTerminal,
    /// A Group with these children is not converted to an Issue.
    GroupWithChildren(Vec<EntityId>),
    /// The transition does not lead anywhere from this kind and lifecycle: a Group is never
    /// started or released, and each operation starts from its own lifecycles.
    NotApplicable {
        operation: Operation,
        kind: Kind,
        lifecycle: Lifecycle,
    },
    /// These ancestors are not adopted (NotStarted), or are missing or conflicted so they
    /// cannot be known to be; the walk stops at a missing one.
    AncestorsNotAdopted(Vec<EntityId>),
    /// These dependencies are not Completed, or are missing or conflicted.
    DependenciesNotCompleted(Vec<EntityId>),
    /// The `dependencies` of `ancestor` are not Completed, so nothing under it starts.
    AncestorDependenciesNotCompleted {
        ancestor: EntityId,
        dependencies: Vec<EntityId>,
    },
    /// These children are not terminal, so the Entity is neither completed nor cancelled.
    ChildrenNotEnded(Vec<EntityId>),
    /// The Group is InProgress through these children, which are InProgress or Completed, so
    /// it is not withdrawn.
    WorkingGroupWithdrawn(Vec<EntityId>),
    /// A Group with InProgress or Completed work below it is accepted only under adopted
    /// ancestors; these are not.
    StartedWorkNeedsAdoptedAncestors(Vec<EntityId>),
    /// These Completed Entities depend on the Entity, so it is not reopened before them.
    CompletedDependents(Vec<EntityId>),
}

impl Refusal {
    /// The Entities the refusal points at, in the order it names them. The Entity operated on
    /// is listed only where it is among them (a conflict, a missing Entity, a violation or a
    /// cycle it is on); the destination of a move to an unadopted line is listed only when it
    /// is among `unadopted`.
    pub fn related(&self) -> Vec<&EntityId> {
        match self {
            Self::Conflicted(ids)
            | Self::GroupWithChildren(ids)
            | Self::AncestorsNotAdopted(ids)
            | Self::DependenciesNotCompleted(ids)
            | Self::ChildrenNotEnded(ids)
            | Self::WorkingGroupWithdrawn(ids)
            | Self::StartedWorkNeedsAdoptedAncestors(ids)
            | Self::CompletedDependents(ids) => ids.iter().collect(),
            Self::AncestorDependenciesNotCompleted {
                ancestor,
                dependencies,
            } => std::iter::once(ancestor).chain(dependencies).collect(),
            Self::StartedWorkNeedsAdoptedDestination { unadopted, .. } => {
                unadopted.iter().collect()
            }
            Self::EntityConflicted(id)
            | Self::Missing(id)
            | Self::ParentClosed(id)
            | Self::DestinationNotOpenGroup(id)
            | Self::ContainmentCycle { destination: id }
            | Self::DependencyOnAncestor { target: id, .. }
            | Self::DependencyOnDescendant { target: id, .. } => vec![id],
            Self::NewViolations(violations) => violations.iter().map(|(id, _)| id).collect(),
            Self::CompletionCycle {
                dependent,
                predecessor,
            } => vec![dependent, predecessor],
            Self::SelfContainment
            | Self::SelfDependency
            | Self::CompletedDependenciesFixed
            | Self::ConvertInProgress
            | Self::ConvertTerminal
            | Self::NotApplicable { .. } => Vec::new(),
        }
    }
}

fn joined(ids: &[EntityId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflicted(ids) => write!(
                f,
                "conflicted Entities block every operation except resolve and note add; list their heads with axon resolve: {}",
                joined(ids)
            ),
            Self::EntityConflicted(id) => write!(f, "Entity {id} is conflicted; resolve it first"),
            Self::Missing(id) => write!(f, "missing Entity {id}"),
            Self::ParentClosed(_) | Self::DestinationNotOpenGroup(_) => {
                f.write_str("parent must be an unfinished Group")
            }
            Self::SelfContainment => f.write_str("a Group does not contain itself"),
            Self::ContainmentCycle { .. } => f.write_str("containment cycle"),
            Self::StartedWorkNeedsAdoptedDestination { .. } => f.write_str(
                "InProgress or Completed work moves only under a Group that is adopted (NotStarted) along with all of its ancestors",
            ),
            Self::NewViolations(violations) => write!(
                f,
                "the change would add a structural violation: {}",
                violations
                    .iter()
                    .map(|(id, kind)| format!("{id} ({})", kind.label()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::CompletionCycle {
                dependent,
                predecessor,
            } => write!(
                f,
                "the change would put {dependent} waiting on {predecessor} on a completion cycle"
            ),
            Self::SelfDependency => f.write_str("an Entity does not depend on itself"),
            Self::DependencyOnAncestor { entity, target } => {
                write!(f, "{target} is an ancestor of {entity}")
            }
            Self::DependencyOnDescendant { entity, target } => {
                write!(f, "{target} is a descendant of {entity}")
            }
            Self::CompletedDependenciesFixed => f.write_str("Completed dependencies are fixed"),
            Self::ConvertInProgress => {
                f.write_str("release the InProgress Issue before converting it")
            }
            Self::ConvertTerminal => f.write_str(
                "a Completed or Cancelled Entity is not converted; reopen or reconsider it first",
            ),
            Self::GroupWithChildren(children) => write!(
                f,
                "a Group with children is not converted to an Issue: {}",
                joined(children)
            ),
            Self::NotApplicable {
                operation,
                kind,
                lifecycle,
            } => match (kind, operation, lifecycle) {
                (Kind::Group, Operation::Start, _) => f.write_str(
                    "a Group is not started directly: it is InProgress while a direct child is InProgress or Completed, and its saved lifecycle does not change",
                ),
                (Kind::Group, Operation::Release, _) => f.write_str(
                    "a Group is not released directly: it stops being InProgress when no direct child is InProgress or Completed, and its saved lifecycle does not change",
                ),
                (Kind::Group, Operation::Complete | Operation::Cancel, Lifecycle::InProgress) => {
                    write!(f, "cannot {operation:?} a Group from {lifecycle:?}")
                }
                _ => write!(f, "cannot {operation:?} from {lifecycle:?}"),
            },
            Self::AncestorsNotAdopted(_) => {
                f.write_str("all ancestor Groups must be adopted (NotStarted)")
            }
            Self::DependenciesNotCompleted(_) => f.write_str("dependencies must be Completed"),
            Self::AncestorDependenciesNotCompleted { ancestor, .. } => {
                write!(f, "dependencies of ancestor {ancestor} must be Completed")
            }
            Self::ChildrenNotEnded(_) => f.write_str("all children must be terminal"),
            Self::WorkingGroupWithdrawn(_) => f.write_str(
                "a Group that is InProgress through its children cannot be withdrawn",
            ),
            Self::StartedWorkNeedsAdoptedAncestors(_) => f.write_str(
                "a Group with InProgress or Completed work below it is accepted only under adopted ancestors",
            ),
            Self::CompletedDependents(dependents) => write!(
                f,
                "Completed dependents must be reopened first: {}",
                joined(dependents)
            ),
        }
    }
}
