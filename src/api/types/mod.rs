//! Suno request and response schemas, grouped by endpoint domain.

mod account;
mod clip;
mod clip_info;
mod clip_mutation;
mod feed;
mod generation;
mod lyrics;
mod metadata;
mod operations;
mod persona;
mod playlist;
mod prompts;
mod upload;

#[cfg(test)]
pub use account::MaxLengths;
pub use account::{BillingInfo, Model};
pub use clip::Clip;
pub use clip_info::{
    ClipAttribution, ClipComments, ClipInfo, ClipInfoSupplementalError,
    DirectChildrenCountResponse, SimilarClipsResponse,
};
pub use clip_mutation::{ClipReaction, ClipTrashRequest, SetClipReactionRequest};
pub use feed::{FeedFilters, FeedResponse, FeedV3Request};
pub use generation::{ControlSliders, GenerateRequest, GenerateResponse, LastTagsGeneration};
pub use lyrics::{AlignedWord, LyricsResult, LyricsSubmitResponse};
pub use metadata::{SetMetadataRequest, SetVisibilityRequest};
pub use operations::ConcatRequest;
pub use persona::{
    CreatePersonaRequest, EditPersonaRequest, PersonaClipsResponse, PersonaInfo,
    PersonaListResponse, PersonaListScope, ProcessedClipInfo,
};
pub use persona::{TogglePersonaLoveResponse, TrashPersonasRequest, TrashPersonasResponse};
pub use playlist::{
    CreatePlaylistRequest, PlaylistInfo, PlaylistListResponse, PlaylistReaction,
    PlaylistReorderRequest, PlaylistTrackMutationFailure, PlaylistTrackMutationReport,
    PlaylistTracksRequest, SetPlaylistCoverRequest, SetPlaylistMetadataRequest,
    SetPlaylistReactionRequest, SetPlaylistVisibilityRequest, TrashPlaylistRequest,
};
pub use prompts::{PromptUpsampleRequest, PromptUpsampleResponse};
pub use upload::{
    AudioUploadInitResponse, AudioUploadStatus, CreateAudioUploadRequest, CreateAudioUploadSpec,
    CreateImageUploadRequest, FinishAudioUploadRequest, FinishImageUploadResponse,
    ImageUploadInitResponse, InitializeAudioClipRequest, InitializeAudioClipResponse,
};
