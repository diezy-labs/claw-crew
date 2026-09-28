package errors

import (
	"errors"
	"fmt"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Layer identifies where an error originated in the architecture
type Layer string

const (
	LayerDelivery   Layer = "DELIVERY"
	LayerService    Layer = "SERVICE"
	LayerRepository Layer = "REPOSITORY"
	LayerExternal   Layer = "EXTERNAL"
	LayerInternal   Layer = "INTERNAL"
)

// Code defines standard application error codes
type Code string

const (
	CodeNotFound           Code = "NOT_FOUND"
	CodeInvalidArgument    Code = "INVALID_ARGUMENT"
	CodeInternal           Code = "INTERNAL_SERVER_ERROR"
	CodeTimeout            Code = "TIMEOUT"
	CodeUnauthorized       Code = "UNAUTHORIZED"
	CodePermissionDenied   Code = "PERMISSION_DENIED"
	CodeFailedPrecondition Code = "FAILED_PRECONDITION"
	CodeToolFailed         Code = "TOOL_EXECUTION_FAILED"
	CodeLLMStreamError     Code = "LLM_STREAM_ERROR"
	CodeUnavailable        Code = "SERVICE_UNAVAILABLE"
)

// AppError represents a structured error conforming to Clean Architecture
type AppError struct {
	Code      Code   `json:"code"`
	Message   string `json:"message"`
	Layer     Layer  `json:"layer"`
	RequestID string `json:"request_id,omitempty"`
	Retryable bool   `json:"retryable,omitempty"`
	Err       error  `json:"-"`
}

// ErrorDetail describes the error payload inside a standard ErrorEnvelope
type ErrorDetail struct {
	Code      Code   `json:"code"`
	Message   string `json:"message"`
	Layer     Layer  `json:"layer"`
	RequestID string `json:"request_id,omitempty"`
	Retryable bool   `json:"retryable"`
}

// ErrorEnvelope standardizes cross-layer error payloads (REST, SSE, IPC)
type ErrorEnvelope struct {
	Error ErrorDetail `json:"error"`
}

func (e *AppError) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("[%s][%s] %s: %v", e.Layer, e.Code, e.Message, e.Err)
	}
	return fmt.Sprintf("[%s][%s] %s", e.Layer, e.Code, e.Message)
}

// Unwrap supports errors.Is and errors.As inspection
func (e *AppError) Unwrap() error {
	return e.Err
}

// WithRequestID associates an incoming or generated request ID with the error
func (e *AppError) WithRequestID(reqID string) *AppError {
	e.RequestID = reqID
	return e
}

// WithRetryable explicitly marks whether the operation can be safely retried
func (e *AppError) WithRetryable(retryable bool) *AppError {
	e.Retryable = retryable
	return e
}

// ToEnvelope converts the error into a standardized cross-layer JSON envelope
func (e *AppError) ToEnvelope() ErrorEnvelope {
	return ErrorEnvelope{
		Error: ErrorDetail{
			Code:      e.Code,
			Message:   e.Message,
			Layer:     e.Layer,
			RequestID: e.RequestID,
			Retryable: e.Retryable,
		},
	}
}

// New creates an AppError without an underlying root error
func New(code Code, message string, layer Layer) *AppError {
	retryable := code == CodeTimeout || code == CodeUnavailable
	return &AppError{
		Code:      code,
		Message:   message,
		Layer:     layer,
		Retryable: retryable,
	}
}

// Wrap wraps an existing error with Clean Architecture context
func Wrap(err error, code Code, message string, layer Layer) *AppError {
	if err == nil {
		return nil
	}
	retryable := code == CodeTimeout || code == CodeUnavailable
	return &AppError{
		Code:      code,
		Message:   message,
		Layer:     layer,
		Retryable: retryable,
		Err:       err,
	}
}

// ToGRPCStatus maps AppError to standard gRPC status error
func ToGRPCStatus(err error) error {
	if err == nil {
		return nil
	}

	var appErr *AppError
	if errors.As(err, &appErr) {
		var grpcCode codes.Code
		switch appErr.Code {
		case CodeNotFound:
			grpcCode = codes.NotFound
		case CodeInvalidArgument:
			grpcCode = codes.InvalidArgument
		case CodeTimeout:
			grpcCode = codes.DeadlineExceeded
		case CodeUnauthorized:
			grpcCode = codes.Unauthenticated
		case CodeToolFailed:
			grpcCode = codes.FailedPrecondition
		case CodeLLMStreamError:
			grpcCode = codes.Unavailable
		case CodeUnavailable:
			grpcCode = codes.Unavailable
		default:
			grpcCode = codes.Internal
		}
		return status.Errorf(grpcCode, "%s: %s", appErr.Code, appErr.Message)
	}

	return status.Errorf(codes.Unknown, "%v", err)
}
