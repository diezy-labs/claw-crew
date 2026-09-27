package errors

import (
	"errors"
	"fmt"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Layer mengidentifikasi di mana error pertama kali terjadi
type Layer string

const (
	LayerDelivery   Layer = "DELIVERY"
	LayerService    Layer = "SERVICE"
	LayerRepository Layer = "REPOSITORY"
	LayerExternal   Layer = "EXTERNAL"
	LayerInternal   Layer = "INTERNAL"
)

// Code mendefinisikan kode error standar aplikasi
type Code string

const (
	CodeNotFound       Code = "NOT_FOUND"
	CodeInvalidArgument Code = "INVALID_ARGUMENT"
	CodeInternal       Code = "INTERNAL_SERVER_ERROR"
	CodeTimeout        Code = "TIMEOUT"
	CodeUnauthorized   Code = "UNAUTHORIZED"
	CodeToolFailed     Code = "TOOL_EXECUTION_FAILED"
	CodeLLMStreamError Code = "LLM_STREAM_ERROR"
)

// AppError merepresentasikan structured error sesuai Clean Architecture
type AppError struct {
	Code    Code   `json:"code"`
	Message string `json:"message"`
	Layer   Layer  `json:"layer"`
	Err     error  `json:"-"`
}

func (e *AppError) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("[%s][%s] %s: %v", e.Layer, e.Code, e.Message, e.Err)
	}
	return fmt.Sprintf("[%s][%s] %s", e.Layer, e.Code, e.Message)
}

// Unwrap mendukung errors.Is dan errors.As
func (e *AppError) Unwrap() error {
	return e.Err
}

// New membuat AppError baru tanpa underlying error
func New(code Code, message string, layer Layer) *AppError {
	return &AppError{
		Code:    code,
		Message: message,
		Layer:   layer,
	}
}

// Wrap membungkus existing error dengan konteks Clean Architecture
func Wrap(err error, code Code, message string, layer Layer) *AppError {
	if err == nil {
		return nil
	}
	return &AppError{
		Code:    code,
		Message: message,
		Layer:   layer,
		Err:     err,
	}
}

// ToGRPCStatus memetakan AppError ke gRPC status error
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
		default:
			grpcCode = codes.Internal
		}
		return status.Errorf(grpcCode, "%s: %s", appErr.Code, appErr.Message)
	}

	return status.Errorf(codes.Unknown, "%v", err)
}
