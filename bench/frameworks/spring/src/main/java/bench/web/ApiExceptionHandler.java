package bench.web;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.springframework.context.MessageSourceResolvable;
import org.springframework.core.MethodParameter;
import org.springframework.http.ResponseEntity;
import org.springframework.http.converter.HttpMessageNotReadableException;
import org.springframework.validation.FieldError;
import org.springframework.validation.method.ParameterErrors;
import org.springframework.validation.method.ParameterValidationResult;
import org.springframework.web.bind.MethodArgumentNotValidException;
import org.springframework.web.bind.annotation.ExceptionHandler;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestControllerAdvice;
import org.springframework.web.method.annotation.HandlerMethodValidationException;
import org.springframework.web.method.annotation.MethodArgumentTypeMismatchException;

/** Validation and binding failures as {@code 400 {"message": ..., "errors": {field: [messages]}}}. */
@RestControllerAdvice
public class ApiExceptionHandler {

    public record ValidationError(String message, Map<String, List<String>> errors) {
    }

    /** {@code @Valid @RequestBody} failures. */
    @ExceptionHandler(MethodArgumentNotValidException.class)
    ResponseEntity<ValidationError> invalidBody(MethodArgumentNotValidException e) {
        Map<String, List<String>> errors = new LinkedHashMap<>();
        for (FieldError error : e.getBindingResult().getFieldErrors()) {
            add(errors, snakeCase(error.getField()), error);
        }
        return badRequest(errors);
    }

    /** Constraint failures on handler method parameters, such as {@code @Min} on a query parameter. */
    @ExceptionHandler(HandlerMethodValidationException.class)
    ResponseEntity<ValidationError> invalidParameters(HandlerMethodValidationException e) {
        Map<String, List<String>> errors = new LinkedHashMap<>();
        for (ParameterValidationResult result : e.getParameterValidationResults()) {
            if (result instanceof ParameterErrors parameterErrors) {
                for (FieldError error : parameterErrors.getFieldErrors()) {
                    add(errors, snakeCase(error.getField()), error);
                }
            } else {
                String name = parameterName(result.getMethodParameter());
                result.getResolvableErrors().forEach(error -> add(errors, name, error));
            }
        }
        return badRequest(errors);
    }

    /** A query parameter that does not convert, such as {@code page=abc}. */
    @ExceptionHandler(MethodArgumentTypeMismatchException.class)
    ResponseEntity<ValidationError> typeMismatch(MethodArgumentTypeMismatchException e) {
        String name = parameterName(e.getParameter());
        Class<?> type = e.getRequiredType();
        boolean integer = type == int.class || type == Integer.class || type == long.class || type == Long.class;
        return badRequest(Map.of(name, List.of(integer ? "must be an integer" : "has an invalid value")));
    }

    /** Malformed JSON, or a value of the wrong JSON type (for example 10.5 for an integer). */
    @ExceptionHandler(HttpMessageNotReadableException.class)
    ResponseEntity<ValidationError> unreadable(HttpMessageNotReadableException e) {
        return ResponseEntity.badRequest().body(new ValidationError("The request body is not valid JSON for this endpoint.", Map.of()));
    }

    private static ResponseEntity<ValidationError> badRequest(Map<String, List<String>> errors) {
        return ResponseEntity.badRequest().body(new ValidationError("The given data was invalid.", errors));
    }

    private static void add(Map<String, List<String>> errors, String field, MessageSourceResolvable error) {
        errors.computeIfAbsent(field, key -> new ArrayList<>()).add(error.getDefaultMessage());
    }

    private static String parameterName(MethodParameter parameter) {
        RequestParam requestParam = parameter.getParameterAnnotation(RequestParam.class);
        if (requestParam != null && !requestParam.name().isEmpty()) {
            return requestParam.name();
        }
        String name = parameter.getParameterName();
        return name == null ? "parameter" : name;
    }

    private static String snakeCase(String name) {
        return name.replaceAll("([a-z0-9])([A-Z])", "$1_$2").toLowerCase();
    }
}
