<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Bundle\FrameworkBundle\Controller;

use Psr\Container\ContainerInterface;
use Psr\Link\EvolvableLinkInterface;
use Psr\Link\LinkInterface;
use Symfony\Component\DependencyInjection\Exception\ServiceNotFoundException;
use Symfony\Component\DependencyInjection\ParameterBag\ContainerBagInterface;
use Symfony\Component\Form\Extension\Core\Type\FormType;
use Symfony\Component\Form\Flow\FormFlowInterface;
use Symfony\Component\Form\Flow\FormFlowTypeInterface;
use Symfony\Component\Form\FormBuilderInterface;
use Symfony\Component\Form\FormFactoryInterface;
use Symfony\Component\Form\FormInterface;
use Symfony\Component\HttpFoundation\BinaryFileResponse;
use Symfony\Component\HttpFoundation\Exception\SessionNotFoundException;
use Symfony\Component\HttpFoundation\JsonResponse;
use Symfony\Component\HttpFoundation\RedirectResponse;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\RequestStack;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\HttpFoundation\ResponseHeaderBag;
use Symfony\Component\HttpFoundation\Session\FlashBagAwareSessionInterface;
use Symfony\Component\HttpFoundation\StreamedResponse;
use Symfony\Component\HttpKernel\Exception\NotFoundHttpException;
use Symfony\Component\HttpKernel\HttpKernelInterface;
use Symfony\Component\Routing\Generator\UrlGeneratorInterface;
use Symfony\Component\Routing\RouterInterface;
use Symfony\Component\Security\Core\Authentication\Token\Storage\TokenStorageInterface;
use Symfony\Component\Security\Core\Authorization\AccessDecision;
use Symfony\Component\Security\Core\Authorization\AuthorizationCheckerInterface;
use Symfony\Component\Security\Core\Exception\AccessDeniedException;
use Symfony\Component\Security\Core\User\UserInterface;
use Symfony\Component\Security\Csrf\CsrfToken;
use Symfony\Component\Security\Csrf\CsrfTokenManagerInterface;
use Symfony\Component\Serializer\SerializerInterface;
use Symfony\Component\WebLink\EventListener\AddLinkHeaderListener;
use Symfony\Component\WebLink\GenericLinkProvider;
use Symfony\Component\WebLink\HttpHeaderSerializer;
use Symfony\Contracts\Service\Attribute\Required;
use Symfony\Contracts\Service\ServiceSubscriberInterface;
use Twig\Environment;

/**
 * Provides shortcuts for HTTP-related features in controllers.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
abstract class AbstractController implements ServiceSubscriberInterface
{
    protected ContainerInterface $container;

    #[Required]
    public function setContainer(ContainerInterface $container): ?ContainerInterface
    {}

    public static function getSubscribedServices(): array
    {}

    /**
     * Gets a container parameter by its name.
     */
    protected function getParameter(string $name): array|bool|string|int|float|\UnitEnum|null
    {}

    /**
     * Generates a URL from the given parameters.
     *
     * @see UrlGeneratorInterface
     */
    protected function generateUrl(string $route, array $parameters = [], int $referenceType = UrlGeneratorInterface::ABSOLUTE_PATH): string
    {}

    /**
     * Forwards the request to another controller.
     *
     * @param string $controller The controller name (a string like "App\Controller\PostController::index" or "App\Controller\PostController" if it is invokable)
     */
    protected function forward(string $controller, array $path = [], array $query = []): Response
    {}

    /**
     * Returns a RedirectResponse to the given URL.
     *
     * @param int $status The HTTP status code (302 "Found" by default)
     */
    protected function redirect(string $url, int $status = 302): RedirectResponse
    {}

    /**
     * Returns a RedirectResponse to the given route with the given parameters.
     *
     * @param int $status The HTTP status code (302 "Found" by default)
     */
    protected function redirectToRoute(string $route, array $parameters = [], int $status = 302): RedirectResponse
    {}

    /**
     * Returns a JsonResponse that uses the serializer component if enabled, or json_encode.
     *
     * @param int $status The HTTP status code (200 "OK" by default)
     */
    protected function json(mixed $data, int $status = 200, array $headers = [], array $context = []): JsonResponse
    {}

    /**
     * Returns a BinaryFileResponse object with original or customized file name and disposition header.
     */
    protected function file(\SplFileInfo|string $file, ?string $fileName = null, string $disposition = ResponseHeaderBag::DISPOSITION_ATTACHMENT): BinaryFileResponse
    {}

    /**
     * Adds a flash message to the current session for type.
     *
     * @throws \LogicException
     */
    protected function addFlash(string $type, mixed $message): void
    {}

    /**
     * Checks if the attribute is granted against the current authentication token and optionally supplied subject.
     *
     * @throws \LogicException
     */
    protected function isGranted(mixed $attribute, mixed $subject = null): bool
    {}

    /**
     * Checks if the attribute is granted against the current authentication token and optionally supplied subject.
     */
    protected function getAccessDecision(mixed $attribute, mixed $subject = null): AccessDecision
    {}

    /**
     * Throws an exception unless the attribute is granted against the current authentication token and optionally
     * supplied subject.
     *
     * @throws AccessDeniedException
     */
    protected function denyAccessUnlessGranted(mixed $attribute, mixed $subject = null, string $message = 'Access Denied.'): void
    {}

    /**
     * Returns a rendered view.
     *
     * Forms found in parameters are auto-cast to form views.
     */
    protected function renderView(string $view, array $parameters = []): string
    {}

    /**
     * Returns a rendered block from a view.
     *
     * Forms found in parameters are auto-cast to form views.
     */
    protected function renderBlockView(string $view, string $block, array $parameters = []): string
    {}

    /**
     * Renders a view.
     *
     * If an invalid form is found in the list of parameters, a 422 status code is returned.
     * Forms found in parameters are auto-cast to form views.
     */
    protected function render(string $view, array $parameters = [], ?Response $response = null): Response
    {}

    /**
     * Renders a block in a view.
     *
     * If an invalid form is found in the list of parameters, a 422 status code is returned.
     * Forms found in parameters are auto-cast to form views.
     */
    protected function renderBlock(string $view, string $block, array $parameters = [], ?Response $response = null): Response
    {}

    /**
     * Streams a view.
     */
    protected function stream(string $view, array $parameters = [], ?StreamedResponse $response = null): StreamedResponse
    {}

    /**
     * Returns a NotFoundHttpException.
     *
     * This will result in a 404 response code. Usage example:
     *
     *     throw $this->createNotFoundException('Page not found!');
     */
    protected function createNotFoundException(string $message = 'Not Found', ?\Throwable $previous = null): NotFoundHttpException
    {}

    /**
     * Returns an AccessDeniedException.
     *
     * This will result in a 403 response code. Usage example:
     *
     *     throw $this->createAccessDeniedException('Unable to access this page!');
     *
     * @throws \LogicException If the Security component is not available
     */
    protected function createAccessDeniedException(string $message = 'Access Denied.', ?\Throwable $previous = null): AccessDeniedException
    {}

    /**
     * Creates and returns a Form instance from the type of the form.
     *
     * @return ($type is class-string<FormFlowTypeInterface> ? FormFlowInterface : FormInterface)
     */
    protected function createForm(string $type, mixed $data = null, array $options = []): FormInterface
    {}

    /**
     * Creates and returns a form builder instance.
     */
    protected function createFormBuilder(mixed $data = null, array $options = []): FormBuilderInterface
    {}

    /**
     * Get a user from the Security Token Storage.
     *
     * @throws \LogicException If SecurityBundle is not available
     *
     * @see TokenInterface::getUser()
     */
    protected function getUser(): ?UserInterface
    {}

    /**
     * Checks the validity of a CSRF token.
     *
     * @param string      $id    The id used when generating the token
     * @param string|null $token The actual token sent with the request that should be validated
     */
    protected function isCsrfTokenValid(string $id, #[\SensitiveParameter] ?string $token): bool
    {}

    /**
     * Adds a Link HTTP header to the current response.
     *
     * @see https://tools.ietf.org/html/rfc5988
     */
    protected function addLink(Request $request, LinkInterface $link): void
    {}

    /**
     * @param LinkInterface[] $links
     */
    protected function sendEarlyHints(iterable $links = [], ?Response $response = null): Response
    {}

    private function doRenderView(string $view, ?string $block, array $parameters, string $method): string
    {}

    private function doRender(string $view, ?string $block, array $parameters, ?Response $response, string $method): Response
    {}
}
